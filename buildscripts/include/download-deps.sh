#!/bin/bash -e

. ./include/depinfo.sh

[ -z "$WGET" ] && WGET=wget

# fetch_tarball <name> <url> <sha256>
# Downloads to a temp file, verifies the checksum, then unpacks into ./<name>.
# Nothing is left in ./<name> on failure, as the dependency would be skipped next run.
fetch_tarball () {
	local name=$1 url=$2 want=$3 tmp got
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
		echo "  If this is a version bump, update sha256_$name in include/depinfo.sh." >&2
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

mkdir -p deps && cd deps

# mbedtls
[ ! -d mbedtls ] && git clone --depth 1 --branch v$v_mbedtls --recurse-submodules https://github.com/Mbed-TLS/mbedtls.git mbedtls

# dav1d
[ ! -d dav1d ] && git clone --depth 1 --branch $v_dav1d https://code.videolan.org/videolan/dav1d.git dav1d

# ffmpeg
[ ! -d ffmpeg ] && git clone --depth 1 --branch n$v_ffmpeg https://github.com/FFmpeg/FFmpeg.git ffmpeg

# freetype2
[ ! -d freetype ] && git clone --depth 1 --branch VER-$v_freetype https://gitlab.freedesktop.org/freetype/freetype.git freetype

# fribidi
[ ! -d fribidi ] && git clone --depth 1 --branch v$v_fribidi https://github.com/fribidi/fribidi.git fribidi

# harfbuzz
[ ! -d harfbuzz ] && git clone --depth 1 --branch $v_harfbuzz https://github.com/harfbuzz/harfbuzz.git harfbuzz

# libunibreak
[ ! -d libunibreak ] && fetch_tarball libunibreak \
	https://github.com/adah1972/libunibreak/releases/download/libunibreak_${v_libunibreak}/libunibreak-${v_libunibreak//_/.}.tar.gz \
	$sha256_libunibreak

# libxml2
[ ! -d libxml2 ] && git clone --depth 1 --branch v$v_libxml2 https://gitlab.gnome.org/GNOME/libxml2.git libxml2

# fontconfig
[ ! -d fontconfig ] && git clone --depth 1 --branch $v_fontconfig https://gitlab.freedesktop.org/fontconfig/fontconfig.git fontconfig

# libass
[ ! -d libass ] && git clone --depth 1 --branch $v_libass https://github.com/libass/libass.git libass

# lua
[ ! -d lua ] && fetch_tarball lua \
	https://www.lua.org/ftp/lua-$v_lua.tar.gz \
	$sha256_lua

[ ! -d libplacebo ] && git clone --depth 1 --branch v$v_libplacebo --recurse-submodules https://code.videolan.org/videolan/libplacebo.git libplacebo

# curl (release tarball, the git repo has no generated configure script)
[ ! -d curl ] && fetch_tarball curl \
	https://curl.se/download/curl-${v_curl//_/.}.tar.gz \
	$sha256_curl

# mpv
[ ! -d mpv ] && git clone --depth 1 --branch v$v_mpv https://github.com/mpv-player/mpv.git mpv

cd ..
