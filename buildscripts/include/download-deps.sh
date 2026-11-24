#!/bin/bash -e
set -e # also when run as "bash download-deps.sh"

. ./include/depinfo.sh

[ -z "$WGET" ] && WGET=wget

# The commit each git tag resolved to when it was recorded (see the file). Absolute, as
# the functions below run in deps/.
tag_commits="$PWD/include/tag-commits.txt"

# DEPS_LIST=1: print every source as "git|<name>|<url>|<tag>" or
# "tarball|<name>|<url>|sha256_<name>" and download nothing (update-checksums.sh reads it)
DEPS_LIST=${DEPS_LIST:-0}

# fetch_tarball <name> <url> <sha256>
# Downloads to a temp file, verifies the checksum, then unpacks into ./<name>.
# Nothing is left in ./<name> on failure, as the dependency would be skipped next run.
fetch_tarball () {
	local name=$1 url=$2 want=$3 tmp got
	if [ "$DEPS_LIST" = 1 ]; then
		printf 'tarball|%s|%s|sha256_%s\n' "$name" "$url" "$name"
		return 0
	fi
	[ -d "$name" ] && return 0
	tmp=$(mktemp "${TMPDIR:-/tmp}/dl-$name.XXXXXX") || return 1
	if ! $WGET "$url" -O "$tmp"; then
		rm -f "$tmp"
		echo "Error: failed to download $name from $url" >&2
		return 1
	fi
	if hash sha256sum 2>/dev/null; then
		got=$(sha256sum "$tmp" | cut -d' ' -f1)
	else
		got=$(shasum -a 256 "$tmp" | cut -d' ' -f1)
	fi
	if [ "$got" != "$want" ]; then
		rm -f "$tmp"
		echo "Error: SHA-256 mismatch for $name ($url)" >&2
		echo "  expected: $want" >&2
		echo "  actual:   $got" >&2
		echo "  If this is a version bump, run ./update-checksums.sh $name (it updates sha256_$name in include/depinfo.sh)." >&2
		return 1
	fi
	mkdir "$name"
	if ! tar -xzf "$tmp" -C "$name" --strip-components=1; then
		rm -rf "$tmp" "$name"
		echo "Error: failed to unpack $name" >&2
		return 1
	fi
	rm -f "$tmp"
}

warn () {
	echo "Warning: $*" >&2
	if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; fi
}

# fetch_git <name> <tag> <url> [git clone options]
# Clones the tag into ./<name> and checks that it is the commit recorded for that tag in
# include/tag-commits.txt: a tag that now points elsewhere (an upstream re-tagged a
# release) fails here, instead of later as a cached build that does not match the source.
# A tag without a record (a version bump, e.g. by Renovate) only warns.
fetch_git () {
	local name=$1 tag=$2 url=$3 got want
	shift 3
	if [ "$DEPS_LIST" = 1 ]; then
		printf 'git|%s|%s|%s\n' "$name" "$url" "$tag"
		return 0
	fi
	[ -d "$name" ] && return 0
	git clone --depth 1 --branch "$tag" "$@" "$url" "$name"
	got=$(git -C "$name" rev-parse HEAD)
	want=$(awk -v n="$name" -v t="$tag" '$1 == n && $2 == t { print $3; exit }' "$tag_commits" 2>/dev/null || true)
	if [ -z "$want" ]; then
		warn "$name: no commit recorded for tag $tag in include/tag-commits.txt (got $got); run ./update-checksums.sh $name to record it"
	elif [ "$got" != "$want" ]; then
		rm -rf "$name"
		echo "Error: $name tag $tag is commit $got, but include/tag-commits.txt recorded $want." >&2
		echo "  The tag was moved upstream. Find out why before trusting it; to accept the new commit," >&2
		echo "  run ./update-checksums.sh --accept-moved-tags $name and bump NATIVE_CACHE_EPOCH in build.yaml." >&2
		return 1
	fi
}

if [ "$DEPS_LIST" != 1 ]; then mkdir -p deps && cd deps; fi

# mbedtls
fetch_git mbedtls "v$v_mbedtls" https://github.com/Mbed-TLS/mbedtls.git --recurse-submodules

# dav1d
fetch_git dav1d "$v_dav1d" https://code.videolan.org/videolan/dav1d.git

# ffmpeg
fetch_git ffmpeg "n$v_ffmpeg" https://github.com/FFmpeg/FFmpeg.git

# freetype2
fetch_git freetype "VER-$v_freetype" https://gitlab.freedesktop.org/freetype/freetype.git

# fribidi
fetch_git fribidi "v$v_fribidi" https://github.com/fribidi/fribidi.git

# harfbuzz
fetch_git harfbuzz "$v_harfbuzz" https://github.com/harfbuzz/harfbuzz.git

# libunibreak
fetch_tarball libunibreak \
	"https://github.com/adah1972/libunibreak/releases/download/libunibreak_${v_libunibreak}/libunibreak-${v_libunibreak//_/.}.tar.gz" \
	"$sha256_libunibreak"

# libxml2
fetch_git libxml2 "v$v_libxml2" https://gitlab.gnome.org/GNOME/libxml2.git

# fontconfig
fetch_git fontconfig "$v_fontconfig" https://gitlab.freedesktop.org/fontconfig/fontconfig.git

# libass
fetch_git libass "$v_libass" https://github.com/libass/libass.git

# lua
fetch_tarball lua \
	"https://www.lua.org/ftp/lua-$v_lua.tar.gz" \
	"$sha256_lua"

fetch_git libplacebo "v$v_libplacebo" https://code.videolan.org/videolan/libplacebo.git --recurse-submodules

# curl (release tarball, the git repo has no generated configure script)
fetch_tarball curl \
	"https://curl.se/download/curl-${v_curl//_/.}.tar.gz" \
	"$sha256_curl"

# mpv
fetch_git mpv "v$v_mpv" https://github.com/mpv-player/mpv.git

if [ "$DEPS_LIST" != 1 ]; then cd ..; fi
