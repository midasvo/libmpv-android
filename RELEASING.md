# Releasing

This fork publishes `io.github.midasvo:libmpv` to Maven Central for [Ephyra](https://github.com/midasvo/ephyra). `main` follows [jarnedemeulemeester/libmpv-android](https://github.com/jarnedemeulemeester/libmpv-android). Our own changes are kept small so upstream merges stay trivial.

## What differs from upstream

Keep this list short. Every extra line is a possible merge conflict.

- `README.md`: the "About this fork" note at the top
- `libmpv/build.gradle.kts`: `coordinates(...)` and the `pom { }` block. The version comes from `-PVERSION_NAME`.
- `.github/workflows/publish.yaml`: repository guard, version from the tag, source tarball, release notes
- `.github/workflows/build.yaml`: `workflow_dispatch` trigger and download retries (`WGET`)
- `buildscripts/scripts/mpv.sh`: `-Dbuild-date=false`, so `libmpv.so` is reproducible (worth sending upstream)
- `.github/workflows/sync-upstream.yaml` and this file

The Kotlin package stays `dev.jdtech.mpv`. Do not rename it.

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

Upstream already runs Renovate on `buildscripts/include/depinfo.sh`, and its bumps reach us through the sync. Renovate is **not** enabled on this fork, to avoid duplicate PRs and conflicts. When we need a version sooner than upstream (as with libplacebo for midasvo/ephyra#652), bump it here by hand in a PR. Upstream's identical bump will then merge without a conflict.

## Cutting a release

Versions are our own semver, starting at `1.1.0`. Raise the minor version for dependency bumps, the patch version for build or wrapper fixes, and the major version for a breaking wrapper API change. Each release's notes list the exact component versions.

1. Make sure `main` is green, and merge a pending upstream sync if you want it in.
2. Tag and push:
   ```sh
   git switch main && git pull
   git tag -a v1.2.0 -m "libmpv 1.2.0"
   git push origin v1.2.0
   ```
3. `publish.yaml` runs in the `release` environment (only `v*` tags can deploy). It builds all four ABIs (about 25 minutes), signs and uploads to Maven Central with automatic release, and creates a **draft** GitHub release with the AAR, `libmpv-<version>-sources.tar.xz` and generated notes.
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

When generating a key on Windows, strip `` as well as `
` from generated passphrases: Git Bash's MinGW `openssl` ends its output with CRLF, and a passphrase ending in `` does not survive the trip into an Actions secret (signing then fails with `checksum mismatch`).

**Rotating the key:** create a new key, upload the public key to both keyservers, and replace the three `SIGNING_*` secrets. Old releases stay valid. To extend instead, run `gpg --quick-set-expire <fpr> 3y`, then re-upload the public key and re-export `SIGNING_KEY`. If the key leaks, publish the revocation certificate to the keyservers and rotate.

**Rotating the Portal token:** generate a new user token on central.sonatype.com and replace `OSSRH_USERNAME` and `OSSRH_PASSWORD`.

## Licence

The AAR is effectively GPL-3.0-or-later. Every release attaches its complete corresponding source (`libmpv-<version>-sources.tar.xz`: this repository at the tag plus every dependency as downloaded and patched). Keep it that way. Apps that ship the AAR, Ephyra included, must show the licences and point to that source.
