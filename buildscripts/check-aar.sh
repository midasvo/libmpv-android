#!/bin/bash
# Sanity-checks a built AAR: every ABI has the expected native libraries (non-empty
# regular files, not dangling symlinks) and the 64-bit ABIs are 16 KiB page aligned.
#
# Usage: check-aar.sh [path-to-aar]
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
aar=${1:-$repo_root/libmpv/build/outputs/aar/libmpv-release.aar}

abis=(armeabi-v7a arm64-v8a x86 x86_64)
aligned_abis=(arm64-v8a x86_64)
libs=(libmpv.so libplayer.so libavcodec.so libavformat.so libavutil.so
	libavfilter.so libswresample.so libswscale.so libc++_shared.so)
min_align=16384

if [ ! -f "$aar" ]; then
	echo "Error: AAR not found: $aar" >&2
	exit 2
fi
for tool in unzip; do
	hash "$tool" 2>/dev/null || { echo "Error: '$tool' is required" >&2; exit 2; }
done
if hash llvm-readelf 2>/dev/null; then
	readelf=llvm-readelf
elif hash readelf 2>/dev/null; then
	readelf=readelf
else
	echo "Error: neither llvm-readelf nor readelf found" >&2
	exit 2
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

unzip -q "$aar" -d "$tmp"

problems=()
summary=()

for abi in "${abis[@]}"; do
	dir=$tmp/jni/$abi
	for lib in "${libs[@]}"; do
		f=$dir/$lib
		if [ ! -e "$f" ] && [ ! -L "$f" ]; then
			problems+=("$abi: missing $lib")
		elif [ -L "$f" ] || [ ! -f "$f" ]; then
			problems+=("$abi: $lib is not a regular file (dangling symlink?)")
		elif [ ! -s "$f" ]; then
			problems+=("$abi: $lib is empty")
		fi
	done

	count=0
	size=0
	if [ -d "$dir" ]; then
		while IFS= read -r f; do
			count=$((count + 1))
			size=$((size + $(wc -c < "$f")))
		done < <(find "$dir" -type f -name '*.so')
	fi
	summary+=("$abi: $count .so files, $((size / 1024)) KiB")
done

for abi in "${aligned_abis[@]}"; do
	dir=$tmp/jni/$abi
	[ -d "$dir" ] || continue
	while IFS= read -r f; do
		[ -s "$f" ] || continue
		# LOAD lines: Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align (-W: one line each)
		aligns=$("$readelf" -lW "$f" 2>/dev/null | awk '$1 == "LOAD" { print $NF }') || aligns=
		if [ -z "$aligns" ]; then
			problems+=("$abi: $(basename "$f"): no LOAD segments found (not a valid ELF?)")
			continue
		fi
		for a in $aligns; do
			if [ $((a)) -lt "$min_align" ]; then
				problems+=("$abi: $(basename "$f"): LOAD segment alignment $a is below $min_align")
				break
			fi
		done
	done < <(find "$dir" -type f -name '*.so')
done

if [ ${#problems[@]} -gt 0 ]; then
	echo "AAR check FAILED for $aar:" >&2
	printf '  - %s\n' "${problems[@]}" >&2
	exit 1
fi

echo "AAR check passed: $aar"
printf '  %s\n' "${summary[@]}"
