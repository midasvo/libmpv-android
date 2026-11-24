#!/bin/bash
# Brings what include/download-deps.sh verifies up to date with the versions in
# include/depinfo.sh, typically after a version bump (Renovate bumps v_curl, v_lua and
# v_libunibreak, and the download fails until sha256_<name> matches):
#
# - tarballs (curl, lua, libunibreak): downloads the tarball of the current version, prints
#   the recorded and the downloaded SHA-256, checks the download against what the project
#   publishes itself where it can (lua.org's checksum list, the GitHub release's asset
#   digest, curl's GPG signature), and writes the new value to sha256_<name> in depinfo.sh;
# - git tags (everything else): resolves the tag with git ls-remote (the commit, also for an
#   annotated tag) and records it in include/tag-commits.txt, which download-deps.sh checks
#   every clone against. A recorded tag that now resolves to another commit is an error:
#   the tag was moved upstream. --accept-moved-tags records the new commit anyway.
#
# Usage: update-checksums.sh [--check] [--accept-moved-tags] [<name>...]
#   <name>   only these dependencies (as in download-deps.sh: curl, lua, mpv, ...)
#   --check  change nothing; exit 1 if a checksum or a tag's commit differs from the record
# Needs curl, git and sha256sum (or shasum); gpg to check curl's signature.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
depinfo=include/depinfo.sh
records=include/tag-commits.txt

check=0 accept_moved=0
names=()
while [ $# -gt 0 ]; do
	case "$1" in
		--check) check=1 ;;
		--accept-moved-tags) accept_moved=1 ;;
		-h|--help) sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p}' "$0"; exit 0 ;;
		-*) echo "update-checksums.sh: unknown option $1" >&2; exit 2 ;;
		*) names+=("$1") ;;
	esac
	shift
done

for tool in curl git; do
	hash "$tool" 2>/dev/null || { echo "update-checksums.sh: '$tool' is required" >&2; exit 2; }
done
sha256 () {
	if hash sha256sum 2>/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# git|<name>|<url>|<tag> and tarball|<name>|<url>|sha256_<name>, from download-deps.sh itself
sources=$(DEPS_LIST=1 bash include/download-deps.sh)
[ -n "$sources" ] || { echo "update-checksums.sh: download-deps.sh listed no sources" >&2; exit 1; }
if [ ${#names[@]} -gt 0 ]; then
	for n in "${names[@]}"; do
		grep -q "^[a-z]*|$n|" <<< "$sources" || { echo "update-checksums.sh: no dependency '$n' in download-deps.sh" >&2; exit 2; }
	done
fi
wanted () {
	[ ${#names[@]} -eq 0 ] && return 0
	local n
	for n in "${names[@]}"; do [ "$n" = "$1" ] && return 0; done
	return 1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

failed=0 differs=0

# What the project publishes about the file: prints one line, returns 0 when it confirms
# the hash, 1 when it contradicts it, 2 when there was nothing to compare with.
verify_tarball () {
	local name=$1 url=$2 file=$3 hash=$4 base published
	base=${url##*/}
	case "$name" in
		lua)
			# https://www.lua.org/ftp/ lists a SHA-256 next to each tarball
			published=$(curl -fsS --max-time 30 https://www.lua.org/ftp/ 2>/dev/null |
				awk -v f="$base" 'index($0, "HREF=\"" f "\"") { found = 1 } found && /CLASS="sum"/ { gsub(/<[^>]*>|[ \t]/, ""); print; exit }') || true
			[ -n "$published" ] || { echo "no checksum for $base found on https://www.lua.org/ftp/: compare by hand"; return 2; }
			[ "$published" = "$hash" ] && { echo "matches the SHA-256 that https://www.lua.org/ftp/ lists"; return 0; }
			echo "DIFFERS from the SHA-256 that https://www.lua.org/ftp/ lists ($published)"; return 1 ;;
		libunibreak)
			# the GitHub release lists a sha256 digest for each asset
			local tag=${url%/*}; tag=${tag##*/}
			published=$(curl -fsS --max-time 30 "https://api.github.com/repos/adah1972/libunibreak/releases/tags/$tag" 2>/dev/null |
				awk -v f="$base" 'index($0, "\"name\": \"" f "\"") { found = 1 } found && /"digest":/ { sub(/.*"sha256:/, ""); sub(/".*/, ""); print; exit }') || true
			[ -n "$published" ] || { echo "no digest for $base on the GitHub release $tag: compare by hand"; return 2; }
			[ "$published" = "$hash" ] && { echo "matches the digest of the GitHub release asset ($tag)"; return 0; }
			echo "DIFFERS from the digest of the GitHub release asset ($published)"; return 1 ;;
		curl)
			# curl signs every tarball (https://curl.se/download.html, "GPG signatures")
			if ! curl -fsS --max-time 30 -o "$file.asc" "$url.asc"; then
				echo "could not download $base.asc: verify the signature by hand"; return 2
			fi
			if ! hash gpg 2>/dev/null; then
				echo "gpg not found: verify $url.asc by hand (gpg --verify $base.asc $base)"; return 2
			fi
			local out rc=0
			out=$(gpg --batch --status-fd 1 --verify "$file.asc" "$file" 2>/dev/null) || rc=$?
			if grep -q '^\[GNUPG:\] GOODSIG' <<< "$out" && [ "$rc" -eq 0 ]; then
				echo "good GPG signature: $(sed -n 's/^\[GNUPG:\] GOODSIG [0-9A-F]* //p' <<< "$out")"; return 0
			elif grep -q '^\[GNUPG:\] BADSIG' <<< "$out"; then
				echo "BAD GPG signature ($base.asc)"; return 1
			fi
			echo "signature not checked (signing key not in your keyring?): see https://curl.se/download.html and run gpg --verify $base.asc $base"; return 2 ;;
		*)
			echo "nothing published to compare with: check the project's release announcement"; return 2 ;;
	esac
}

echo "Tarballs (sha256_<name> in $depinfo):"
while IFS='|' read -r kind name url var; do
	[ "$kind" = tarball ] && wanted "$name" || continue
	old=$(sed -n "s/^$var=//p" "$depinfo")
	file=$tmp/${url##*/}
	echo "  $name: $url"
	if ! curl -fsSL --retry 3 --max-time 300 -o "$file" "$url"; then
		echo "    Error: download failed" >&2
		failed=1
		continue
	fi
	new=$(sha256 "$file")
	echo "    recorded:   ${old:-(none)}"
	echo "    downloaded: $new$([ "$new" = "$old" ] && echo "  (unchanged)")"
	rc=0
	result=$(verify_tarball "$name" "$url" "$file" "$new") || rc=$?
	echo "    verified:   $result"
	if [ "$rc" -eq 1 ]; then
		echo "    Error: not writing $var: the download does not match what the project publishes" >&2
		failed=1
		continue
	fi
	[ "$new" = "$old" ] && continue
	differs=1
	if [ "$check" = 1 ]; then
		echo "    differs from $depinfo (--check: not written)"
		continue
	fi
	awk -v v="$var" -v h="$new" 'index($0, v "=") == 1 { $0 = v "=" h } { print }' "$depinfo" > "$tmp/depinfo" &&
		cat "$tmp/depinfo" > "$depinfo"
	echo "    written to $depinfo$([ "$rc" -eq 2 ] && echo ": check it by hand before committing, as above")"
done <<< "$sources"

echo "Git tags ($records):"
# recorded <name> <field>: field 2 is the tag, 3 the commit (no associative arrays: macOS bash 3.2)
recorded () {
	[ -f "$records" ] || return 0
	awk -v n="$1" -v f="$2" '$1 == n { print $f; exit }' "$records"
}
lines=()
while IFS='|' read -r kind name url tag; do
	[ "$kind" = git ] || continue
	old_tag=$(recorded "$name" 2) old=$(recorded "$name" 3)
	if ! wanted "$name"; then
		[ -n "$old" ] && lines+=("$name $old_tag $old")
		continue
	fi
	refs=$(git ls-remote "$url" "refs/tags/$tag" "refs/tags/$tag^{}") || refs=
	# an annotated tag lists the tag object, then the commit as <tag>^{}
	new=$(awk -v t="refs/tags/$tag^{}" '$2 == t { print $1 }' <<< "$refs")
	[ -n "$new" ] || new=$(awk -v t="refs/tags/$tag" '$2 == t { print $1 }' <<< "$refs")
	if [ -z "$new" ]; then
		echo "  $name $tag: Error: no such tag at $url" >&2
		failed=1
		[ -n "$old" ] && lines+=("$name $old_tag $old")
		continue
	fi
	if [ "$old_tag" = "$tag" ] && [ "$old" = "$new" ]; then
		echo "  $name $tag: $new (unchanged)"
	elif [ "$old_tag" = "$tag" ]; then
		echo "  $name $tag: MOVED: recorded $old, now $new" >&2
		differs=1
		if [ "$accept_moved" = 0 ] || [ "$check" = 1 ]; then
			echo "    Error: the tag was moved upstream. Find out why; to accept it, run with --accept-moved-tags $name and bump NATIVE_CACHE_EPOCH in build.yaml" >&2
			failed=1
			lines+=("$name $old_tag $old")
			continue
		fi
		echo "    accepted (--accept-moved-tags): bump NATIVE_CACHE_EPOCH in build.yaml"
	else
		echo "  $name $tag: $new (new; was ${old_tag:-not recorded}${old:+ $old})"
		differs=1
	fi
	lines+=("$name $tag $new")
done <<< "$sources"

if [ "$check" = 0 ]; then
	{
		echo "# The commit each git tag in include/download-deps.sh resolved to when it was recorded:"
		echo "# <name> <tag> <commit>. download-deps.sh fails when a recorded tag now clones as another"
		echo "# commit (it was moved upstream) and warns when the current tag has no record (a version"
		echo "# bump). Written by ./update-checksums.sh (git ls-remote; for an annotated tag, its commit)."
		printf '%s\n' "${lines[@]}"
	} > "$tmp/records"
	if ! cmp -s "$tmp/records" "$records" 2>/dev/null; then
		cat "$tmp/records" > "$records"
		echo "  written to $records"
	fi
fi

if [ "$failed" = 1 ]; then
	echo "update-checksums.sh: FAILED (see the errors above)" >&2
	exit 1
fi
if [ "$check" = 1 ] && [ "$differs" = 1 ]; then
	echo "update-checksums.sh: --check: the records differ from upstream (see above)" >&2
	exit 1
fi
echo "Done.$([ "$check" = 0 ] && [ "$differs" = 1 ] && echo " Review and commit $depinfo and $records.")"
