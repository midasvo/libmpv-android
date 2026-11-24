#!/bin/bash
# Writes a CycloneDX 1.6 SBOM (JSON) of the native components bundled in the AAR to
# stdout: every library from include/depinfo.sh with version, licence and source, the
# NDK's libc++, the JNI wrapper (licence from its Cargo.toml) and the Rust crates in its
# Cargo.lock (which also lists build-only crates such as proc macros).
#
# Versions come from depinfo.sh. Licences and source locations are kept in the table
# below: when you add a dependency to depinfo.sh, add it here too (the script fails on a
# v_* entry it does not know, so a new dependency cannot be left out silently).
#
# Usage: sbom.sh <aar-version> > libmpv-<version>.cdx.json
# The timestamp is SOURCE_DATE_EPOCH when set, so a given commit gives the same file.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$here/.." && pwd)
version=${1:?usage: sbom.sh <aar-version>}

. "$here/include/depinfo.sh"

# name | depinfo var | licence (SPDX expression, or "name:<free text>") | source kind | source
# Source kinds: git (cloned at the tag <ref>, written after a #) or tarball (sha256_<name>
# in depinfo.sh). @V@ is the depinfo version, @D@ the same with _ and - turned into dots.
components='
mpv|v_mpv|GPL-2.0-or-later|git|https://github.com/mpv-player/mpv.git#v@V@
ffmpeg|v_ffmpeg|GPL-3.0-or-later|git|https://github.com/FFmpeg/FFmpeg.git#n@V@
libplacebo|v_libplacebo|LGPL-2.1-or-later|git|https://code.videolan.org/videolan/libplacebo.git#v@V@
libass|v_libass|ISC|git|https://github.com/libass/libass.git#@V@
lua|v_lua|MIT|tarball|https://www.lua.org/ftp/lua-@V@.tar.gz
curl|v_curl|curl|tarball|https://curl.se/download/curl-@D@.tar.gz
mbedtls|v_mbedtls|Apache-2.0 OR GPL-2.0-or-later|git|https://github.com/Mbed-TLS/mbedtls.git#v@V@
freetype|v_freetype|FTL OR GPL-2.0-or-later|git|https://gitlab.freedesktop.org/freetype/freetype.git#VER-@V@
fontconfig|v_fontconfig|name:fontconfig license (MIT-style)|git|https://gitlab.freedesktop.org/fontconfig/fontconfig.git#@V@
fribidi|v_fribidi|LGPL-2.1-or-later|git|https://github.com/fribidi/fribidi.git#v@V@
harfbuzz|v_harfbuzz|MIT-Modern-Variant|git|https://github.com/harfbuzz/harfbuzz.git#@V@
libunibreak|v_libunibreak|Zlib|tarball|https://github.com/adah1972/libunibreak/releases/download/libunibreak_@V@/libunibreak-@D@.tar.gz
dav1d|v_dav1d|BSD-2-Clause|git|https://code.videolan.org/videolan/dav1d.git#@V@
libxml2|v_libxml2|MIT|git|https://gitlab.gnome.org/GNOME/libxml2.git#v@V@
'
# depinfo.sh entries that are build tools, not bundled code
not_bundled=" v_platform v_sdk v_ndk v_sdk_build_tools v_meson v_gas_preprocessor "

json_licence () {
	case "$1" in
		name:*) printf '{"license":{"name":"%s"}}' "${1#name:}" ;;
		*" "*) printf '{"expression":"%s"}' "$1" ;;
		*) printf '{"license":{"id":"%s"}}' "$1" ;;
	esac
}

# Every v_* in depinfo.sh is either in the table or a known build tool
known=" $(printf '%s\n' "$components" | cut -s -d'|' -f2 | tr '\n' ' ') "
while read -r var; do
	case "$known$not_bundled" in
		*" $var "*) ;;
		*) echo "sbom.sh: $var in depinfo.sh is not in the component table" >&2; exit 1 ;;
	esac
done < <(sed -n 's/^\(v_[a-z0-9_]*\)=.*/\1/p' "$here/include/depinfo.sh")

entries=()

while IFS='|' read -r name var licence kind source; do
	[ -n "$name" ] || continue
	v=${!var:?"$var is not set in depinfo.sh"}
	dotted=${v//_/.}; dotted=${dotted//-/.}
	source=${source//@V@/$v}; source=${source//@D@/$dotted}
	if [ "$kind" = git ]; then
		url=${source%%#*}; ref=${source#*#}
		case "$url" in
			https://github.com/*)
				slug=${url#https://github.com/}; slug=${slug%.git}
				purl="pkg:github/$slug@$ref" ;;
			*) purl="pkg:generic/$name@$dotted?vcs_url=git+$url%40$ref" ;;
		esac
		refs=$(printf '{"type":"vcs","url":"%s","comment":"git tag %s"}' "$url" "$ref")
	else
		shavar=sha256_$name
		sha=${!shavar:?"$shavar is not set in depinfo.sh"}
		purl="pkg:generic/$name@$dotted?download_url=$source&checksum=sha256:$sha"
		refs=$(printf '{"type":"distribution","url":"%s","hashes":[{"alg":"SHA-256","content":"%s"}]}' "$source" "$sha")
	fi
	props=
	if [ -d "$here/patches/$name" ]; then
		for p in "$here/patches/$name"/*; do
			props+="${props:+,}$(printf '{"name":"libmpv-android:patch","value":"buildscripts/patches/%s/%s"}' "$name" "$(basename "$p")")"
		done
		props=",\"properties\":[$props]"
	fi
	entries+=("$(printf '{"type":"library","bom-ref":"%s","name":"%s","version":"%s","licenses":[%s],"purl":"%s","externalReferences":[%s]%s}' \
		"$name" "$name" "$dotted" "$(json_licence "$licence")" "$purl" "$refs" "$props")")
done <<< "$components"

entries+=("$(printf '{"type":"library","bom-ref":"libc++","name":"libc++_shared","version":"%s","description":"LLVM libc++ from Android NDK %s","licenses":[%s],"externalReferences":[{"type":"distribution","url":"https://developer.android.com/ndk/downloads"}]}' \
	"$v_ndk" "$v_ndk" "$(json_licence "Apache-2.0 WITH LLVM-exception")")")

# The wrapper's licence is the one its Cargo workspace declares
crate_toml="$repo_root/libmpv/src/main/rust/Cargo.toml"
player_licence=$(sed -n 's/^license = "\(.*\)"$/\1/p' "$crate_toml" 2>/dev/null | head -n 1)
[ -n "$player_licence" ] || { echo "sbom.sh: no license = \"...\" in $crate_toml" >&2; exit 1; }
entries+=("$(printf '{"type":"library","bom-ref":"player","name":"libplayer","version":"%s","description":"JNI wrapper (libmpv/src/main/rust)","licenses":[%s],"externalReferences":[{"type":"vcs","url":"https://github.com/midasvo/libmpv-android"}]}' \
	"$version" "$(json_licence "$player_licence")")")

# Rust crates statically linked into libplayer.so. Cargo.lock has no licences; they are in
# each crate's Cargo.toml (vendored in the corresponding-source tarball).
lock=${CARGO_LOCK:-$repo_root/libmpv/src/main/rust/Cargo.lock}
if [ -f "$lock" ]; then
	while IFS='|' read -r cname cversion csource csum; do
		case "$csource" in
			registry+https://github.com/rust-lang/crates.io-index|sparse+https://index.crates.io/)
				hashes=
				[ -n "$csum" ] && hashes=$(printf ',"hashes":[{"alg":"SHA-256","content":"%s"}]' "$csum")
				entries+=("$(printf '{"type":"library","bom-ref":"crate:%s@%s","name":"%s","version":"%s","purl":"pkg:cargo/%s@%s"%s,"externalReferences":[{"type":"distribution","url":"https://crates.io/api/v1/crates/%s/%s/download"}]}' \
					"$cname" "$cversion" "$cname" "$cversion" "$cname" "$cversion" "$hashes" "$cname" "$cversion")") ;;
			git+*)
				entries+=("$(printf '{"type":"library","bom-ref":"crate:%s@%s","name":"%s","version":"%s","externalReferences":[{"type":"vcs","url":"%s"}]}' \
					"$cname" "$cversion" "$cname" "$cversion" "${csource#git+}")") ;;
			"") ;; # a workspace member: the wrapper itself
			*) echo "sbom.sh: crate $cname has an unknown source '$csource'" >&2; exit 1 ;;
		esac
	done < <(awk '
		function flush() { if (n != "") print n "|" v "|" s "|" c; n = v = s = c = "" }
		/^\[\[package\]\]/ { flush(); next }
		/^\[/ { flush(); next }
		/^name = /     { n = $3; gsub(/"/, "", n) }
		/^version = /  { v = $3; gsub(/"/, "", v) }
		/^source = /   { s = $3; gsub(/"/, "", s) }
		/^checksum = / { c = $3; gsub(/"/, "", c) }
		END { flush() }' "$lock")
else
	echo "sbom.sh: warning: $lock not found, Rust crates not listed" >&2
fi

if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
	timestamp=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)
else
	timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
fi

printf '{\n"bomFormat":"CycloneDX",\n"specVersion":"1.6",\n"version":1,\n'
printf '"metadata":{"timestamp":"%s","tools":{"components":[{"type":"application","name":"buildscripts/sbom.sh"}]},' "$timestamp"
printf '"component":{"type":"library","bom-ref":"aar","group":"io.github.midasvo","name":"libmpv","version":"%s","licenses":[%s],"purl":"pkg:maven/io.github.midasvo/libmpv@%s?type=aar"}},\n' \
	"$version" "$(json_licence GPL-3.0-or-later)" "$version"
printf '"components":[\n'
for i in "${!entries[@]}"; do
	[ "$i" -gt 0 ] && printf ',\n'
	printf '%s' "${entries[$i]}"
done
printf '\n]\n}\n'
