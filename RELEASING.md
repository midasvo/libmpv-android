# Releasing

This fork publishes `io.github.midasvo:libmpv` to Maven Central for [Ephyra](https://github.com/midasvo/ephyra). `main` follows [jarnedemeulemeester/libmpv-android](https://github.com/jarnedemeulemeester/libmpv-android). Our own changes are kept small so upstream merges stay trivial.

## What differs from upstream

Keep this list short. Every extra line is a possible merge conflict.

- `README.md`: the "About this fork" note at the top
- `libmpv/build.gradle.kts`: `coordinates(...)` and the `pom { }` block (the version comes from `-PVERSION_NAME`), plus the androidTest setup
- `gradle/libs.versions.toml` and `libmpv/src/androidTest`: instrumented smoke test
- `libmpv/src/main/cpp` (the JNI wrapper) and `MPVLib.kt`: observer exception safety, UTF-8 sanitising, `command()` returns the mpv error code, `getPropertyLong`, leak fixes
- `.github/workflows/publish.yaml`: repository guard, tag guard, version from the tag, AAR check, draft release before the Maven Central upload, source tarball, release notes
- `.github/workflows/build.yaml`: `workflow_dispatch` trigger, download retries (`WGET`), AAR check, emulator smoke test
- `buildscripts/scripts/mpv.sh`: `-Dbuild-date=false`, so `libmpv.so` is reproducible (worth sending upstream)
- `buildscripts/include/depinfo.sh`, `download-deps.sh`, `download-sdk.sh`: curl version format (`8_23_0`), sha256-verified tarballs, pinned gas-preprocessor, local-setup fix
- `renovate.json`: one added manager for curl
- `buildscripts/check-aar.sh` and `buildscripts/README.md`
- `.github/workflows/sync-upstream.yaml`, `docs/adr/` and this file
- `site/` and `.github/workflows/pages.yaml`: the landing page at <https://midasvo.nl/libmpv-android/>. Update its version table and snippets when you cut a release.

The wrapper files (`libmpv/src/main/cpp`, `MPVLib.kt`) and the build scripts now differ from upstream too, so they are the likeliest conflict spots in a sync.

The Kotlin package stays `dev.jdtech.mpv`. Do not rename it: see [ADR 0001](docs/adr/0001-keep-the-kotlin-package.md).

## Keeping up with upstream

`sync-upstream.yaml` runs every Monday, or by hand: **Actions → Sync upstream → Run workflow**.

- If upstream has new commits, it merges them into the `upstream-sync` branch, opens or updates a PR to `main`, and starts `build.yaml` on it.
- Merge that PR with **Create a merge commit**, never squash or rebase, so upstream's history stays an ancestor of `main` and the next sync is clean.
- If the merge conflicts, it opens an issue titled "Upstream sync needs a manual merge" that lists the files.

### Upstream conflicts

```sh
git fetch https://github.com/jarnedemeulemeester/libmpv-android.git main
git switch -c upstream-sync origin/main
git merge FETCH_HEAD
# resolve: keep upstream's change and re-apply ours (see the list above)
git push -u origin upstream-sync   # then open a PR, merge with a merge commit
```

The usual conflict is upstream bumping `version = "x.y.z"` in `coordinates(...)`. Keep our line, which reads the version from `VERSION_NAME`.

### Renovate

Renovate runs on this fork with upstream's `renovate.json` plus one added manager for curl, which upstream does not track (curl's tags look like `curl-8_23_0`, so `depinfo.sh` stores `8_23_0`). Keep other edits out of that file: every change is a merge conflict. Upstream runs the same config, so most bumps arrive twice: once from Renovate here, once through the sync PR.

- **Checksums are updated by hand.** A Renovate bump of curl, lua or libunibreak fails at the download step until the matching `sha256_*` in `depinfo.sh` is updated: download the new tarball, check it against the project's published signature or hash where there is one, then run `sha256sum` on it.
- **The same version on both sides** merges without a conflict, whichever lands first.
- **Different versions** (Renovate here is a release ahead) conflict on one line of `depinfo.sh`: keep the higher one.
- **Review Renovate PRs here like any change:** CI builds all four ABIs. A bump of mpv, FFmpeg or libplacebo is a release candidate, so read its notes for the player (see Ephyra's `libmpv-release` skill).

## Cutting a release

Versions are our own semver, starting at `1.1.0`. Raise the minor version for dependency bumps, the patch version for build or wrapper fixes, and the major version for a breaking wrapper API change. Each release's notes list the exact component versions.

1. Make sure `main` is green, and merge a pending upstream sync if you want it in.
2. Tag and push:
   ```sh
   git switch main && git pull
   git tag -a v1.2.0 -m "libmpv 1.2.0"
   git push origin v1.2.0
   ```
3. `publish.yaml` runs in the `release` environment (only `v*` tags can deploy). It refuses tags that are not `vX.Y.Z` or whose commit is not contained in `main`, and versions that are already on Maven Central. It builds all four ABIs (about 25 minutes) and runs the AAR check (`buildscripts/check-aar.sh`) first. It then creates the **draft** GitHub release with the AAR, `libmpv-<version>-sources.tar.xz` and generated notes, and only after that signs and uploads to Maven Central with automatic release.
4. Check the deployment on [central.sonatype.com → Deployments](https://central.sonatype.com/publishing/deployments). It appears on `repo1.maven.org` within about 30 minutes, and in search later.
5. Check the draft release notes, add what changed for the player, and publish the release.
6. Bump Ephyra: `libmpv` in `gradle/libs.versions.toml`, then run `docs/mpv-checklist.md`.

A published version can never be replaced on Maven Central. If something is wrong, release the next patch version.

## Secrets and signing key

The `release` environment holds:

| Secret | What |
|---|---|
| `OSSRH_USERNAME`, `OSSRH_PASSWORD` | Central Portal user token (the names date from OSSRH) |
| `SIGNING_KEY` | ASCII-armoured private key |
| `SIGNING_KEY_ID` | Last 8 hex digits of the fingerprint |
| `SIGNING_PASSWORD` | Key passphrase |

The signing key is a dedicated RSA-4096 key with the user ID `libmpv-android release signing (midasvo)` and a three-year expiry. It is published on keyserver.ubuntu.com and keys.openpgp.org; `gpg --keyserver hkps://keyserver.ubuntu.com --search-keys "libmpv-android release signing"` shows the fingerprint and expiry. The private key, passphrase and revocation certificate are kept offline by the maintainer, not in this repository.

When generating a key on Windows, strip `\r` as well as `\n` from generated passphrases: Git Bash's MinGW `openssl` ends its output with CRLF, and a passphrase ending in `\r` does not survive the trip into an Actions secret (signing then fails with `checksum mismatch`).

**Rotating the key:** create a new key, upload the public key to both keyservers, and replace the three `SIGNING_*` secrets. Old releases stay valid. To extend instead, run `gpg --quick-set-expire <fpr> 3y`, then re-upload the public key and re-export `SIGNING_KEY`. If the key leaks, publish the revocation certificate to the keyservers and rotate.

**Rotating the Portal token:** generate a new user token on central.sonatype.com and replace `OSSRH_USERNAME` and `OSSRH_PASSWORD`.

## Licence

The AAR is effectively GPL-3.0-or-later. Every release attaches its complete corresponding source (`libmpv-<version>-sources.tar.xz`: this repository at the tag plus every dependency as downloaded and patched). Keep it that way. Apps that ship the AAR, Ephyra included, must show the licences and point to that source.
