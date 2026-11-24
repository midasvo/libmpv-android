#!/bin/bash
# Checks that libplayer.so, in every ABI of a built AAR, exports exactly the JNI functions
# that the Kotlin code declares. A declared `external fun` without a matching export is an
# UnsatisfiedLinkError at its first call, on a device, and nothing else would notice; an
# export that nothing declares is dead code or a misspelling of a declared one.
#
# Expected names come from the `native` methods in the AAR's classes.jar (javap -p -s),
# mangled as the JNI specification says: Java_<class>_<method>, '/' as '_', '_' as '_1',
# ';' as '_2', '[' as '_3' and any other character as _0xxxx (so the '$' of a nested
# class, or of a Kotlin `internal` member's name$module, becomes _00024). An overloaded
# native method needs the long name (<short>__<mangled argument descriptor>); others may
# use either. libplayer binds every method by name: it does not call RegisterNatives,
# which this check would not see.
#
# Usage: check-jni.sh [path-to-aar]   (needs a JDK: jar, javap; and readelf or llvm-readelf)
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
aar=${1:-$repo_root/libmpv/build/outputs/aar/libmpv-release.aar}
lib=libplayer.so
abis=(armeabi-v7a arm64-v8a x86 x86_64)

[ -f "$aar" ] || { echo "Error: AAR not found: $aar" >&2; exit 2; }
for tool in javap jar unzip; do
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

# javap and jar are native programs: on Windows (Git Bash) give them Windows paths
native_path() {
	if hash cygpath 2>/dev/null; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
unzip -q "$aar" classes.jar 'jni/*/'"$lib" -d "$tmp" || true
jar=$tmp/classes.jar
[ -f "$jar" ] || { echo "Error: no classes.jar in $aar" >&2; exit 2; }

jar tf "$(native_path "$jar")" | tr -d '\r' \
	| grep '\.class$' | grep -v -e '^META-INF/' -e 'module-info\.class$' \
	| sed -e 's/\.class$//' -e 's#/#.#g' > "$tmp/classes.txt"
[ -s "$tmp/classes.txt" ] || { echo "Error: no classes in $aar" >&2; exit 2; }

xargs javap -p -s -classpath "$(native_path "$jar")" < "$tmp/classes.txt" \
	| tr -d '\r' > "$tmp/javap.txt"

# One line per native method: <short JNI name> <long JNI name> <overloaded 0|1> <Kotlin view>
LC_ALL=C awk '
	BEGIN {
		for (i = 32; i < 127; i++) ord[sprintf("%c", i)] = i
	}
	function mangle(s,    out, i, c) {
		out = ""
		for (i = 1; i <= length(s); i++) {
			c = substr(s, i, 1)
			if (c ~ /[A-Za-z0-9]/) out = out c
			else if (c == "/") out = out "_"
			else if (c == "_") out = out "_1"
			else if (c == ";") out = out "_2"
			else if (c == "[") out = out "_3"
			else out = out sprintf("_0%04x", ord[c])
		}
		return out
	}
	/^[a-z].*(class|interface|enum) [^ ]+.*\{$/ {
		n = split($0, w, " ")
		cls = ""
		for (i = 1; i < n; i++)
			if (w[i] == "class" || w[i] == "interface" || w[i] == "enum") { cls = w[i + 1]; break }
		sub(/<.*$/, "", cls)
		pending = ""
		next
	}
	/^  [^ ].*\(.*\);$/ {
		pre = substr($0, 1, index($0, "(") - 1)
		pending = ""
		if (pre !~ /(^| )native /) next
		n = split(pre, w, " ")
		pending = w[n]
		next
	}
	/^    descriptor: \(/ && pending != "" {
		d = $2
		args = substr(d, 2, index(d, ")") - 2)
		internal = cls
		gsub(/\./, "/", internal)
		short = "Java_" mangle(internal) "_" mangle(pending)
		key = cls "." pending
		count[key]++
		line[++m] = short "\t" short "__" mangle(args) "\t" key "\t" cls "." pending d
		pending = ""
		next
	}
	END {
		for (i = 1; i <= m; i++) {
			split(line[i], f, "\t")
			print f[1] "\t" f[2] "\t" (count[f[3]] > 1 ? 1 : 0) "\t" f[4]
		}
	}
' "$tmp/javap.txt" | LC_ALL=C sort > "$tmp/declared.txt"

declared=$(wc -l < "$tmp/declared.txt")
if [ "$declared" -eq 0 ]; then
	echo "Error: no native methods found in $aar's classes.jar (did javap's output format change?)" >&2
	exit 2
fi

problems=()
summary=()
for abi in "${abis[@]}"; do
	so=$tmp/jni/$abi/$lib
	if [ ! -f "$so" ]; then
		problems+=("$abi: no $lib in the AAR")
		continue
	fi
	# Defined (not UND) dynamic symbols named Java_*
	"$readelf" --dyn-syms -W "$so" \
		| awk '$7 != "UND" && $8 ~ /^Java_/ { sub(/@.*/, "", $8); print $8 }' \
		| LC_ALL=C sort -u > "$tmp/exports-$abi.txt"
	: > "$tmp/used-$abi.txt"
	while IFS=$'\t' read -r short long overloaded what; do
		if grep -qxF "$long" "$tmp/exports-$abi.txt"; then
			echo "$long" >> "$tmp/used-$abi.txt"
		elif [ "$overloaded" = 0 ] && grep -qxF "$short" "$tmp/exports-$abi.txt"; then
			echo "$short" >> "$tmp/used-$abi.txt"
		elif [ "$overloaded" = 1 ]; then
			problems+=("$abi: $what is overloaded and needs $long, which $lib does not export")
		else
			problems+=("$abi: $what is declared native, but $lib does not export $short")
		fi
	done < "$tmp/declared.txt"
	while IFS= read -r sym; do
		problems+=("$abi: $lib exports $sym, which no native method declares (misspelt or left over?)")
	done < <(LC_ALL=C sort -u "$tmp/used-$abi.txt" | LC_ALL=C comm -23 "$tmp/exports-$abi.txt" -)
	summary+=("| $abi | $declared | $(wc -l < "$tmp/exports-$abi.txt") |")
done

if [ ${#problems[@]} -gt 0 ]; then
	echo "JNI check FAILED for $aar:" >&2
	printf '  - %s\n' "${problems[@]}" >&2
	exit 1
fi

echo "JNI check passed: $lib exports exactly the $declared native methods of classes.jar in ${#abis[@]} ABIs"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
	{
		echo "### JNI exports"
		echo
		echo "| ABI | native methods in classes.jar | \`Java_*\` exports of $lib |"
		echo "|---|---|---|"
		printf '%s\n' "${summary[@]}"
		echo
	} >> "$GITHUB_STEP_SUMMARY"
fi
