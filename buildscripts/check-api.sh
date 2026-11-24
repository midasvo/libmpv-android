#!/bin/bash
# The public API of the library as a checked-in contract: libmpv/api/libmpv.api is a
# sorted dump, made with the JDK's javap, of every public and protected class, method
# and field in the release AAR's classes.jar. A change to it is a change to what apps
# compiled against the library link to, so it must be deliberate and reviewed: lines
# that disappear or change break binary compatibility, added lines do not.
#
# Usage: check-api.sh [--update] [path-to-aar-or-classes.jar]
#   (default input: libmpv/build/outputs/aar/libmpv-release.aar, built by
#   ./gradlew :libmpv:assembleRelease)
#   without --update: fails with a diff when the built classes differ from the file
#   with --update:    rewrites the file from the built classes
#
# What is left out, because apps cannot use it:
# - classes that are not public in the bytecode, and everything in
#   dev.jdtech.mpv.internal (Kotlin `internal` classes are public in bytecode);
# - members whose name has a '$', other than a trailing `$default` (the synthetic
#   method behind default arguments, which callers do link to): Kotlin's `internal`
#   members (name$module) and synthetic accessors (access$name).
# Constant values are included (-constants): callers compile them in.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
api_file=$repo_root/libmpv/api/libmpv.api

mode=check
if [ "${1:-}" = "--update" ]; then
	mode=update
	shift
fi
input=${1:-$repo_root/libmpv/build/outputs/aar/libmpv-release.aar}

for tool in javap jar; do
	hash "$tool" 2>/dev/null || { echo "Error: '$tool' (from a JDK) is required" >&2; exit 2; }
done
if [ ! -f "$input" ]; then
	echo "Error: $input not found; build it with ./gradlew :libmpv:assembleRelease" >&2
	exit 2
fi

# javap and jar are native programs: on Windows (Git Bash) give them Windows paths
native_path() {
	if hash cygpath 2>/dev/null; then cygpath -w "$1"; else printf '%s\n' "$1"; fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
input_abs=$(cd "$(dirname "$input")" && pwd)/$(basename "$input")

case "$input" in
*.aar)
	(cd "$tmp" && jar xf "$(native_path "$input_abs")" classes.jar)
	jar=$tmp/classes.jar
	[ -f "$jar" ] || { echo "Error: no classes.jar in $input" >&2; exit 2; }
	;;
*.jar) jar=$input_abs ;;
*) echo "Error: expected an .aar or a .jar: $input" >&2; exit 2 ;;
esac

jar tf "$(native_path "$jar")" | tr -d '\r' \
	| grep '\.class$' \
	| grep -v -e '^META-INF/' -e '^dev/jdtech/mpv/internal/' \
	| sed -e 's/\.class$//' -e 's#/#.#g' \
	| LC_ALL=C sort > "$tmp/classes.txt"
if [ ! -s "$tmp/classes.txt" ]; then
	echo "Error: no classes in $input" >&2
	exit 2
fi

# one javap run for all classes: -protected prints public and protected members
xargs javap -protected -constants -classpath "$(native_path "$jar")" < "$tmp/classes.txt" \
	| tr -d '\r' > "$tmp/javap.txt"

# Each class becomes "name<TAB>0<TAB>header" plus "name<TAB>1<TAB>member" lines, so that
# one sort orders the classes by name and the members within each class.
LC_ALL=C awk -F'\t' '
	function member_name(line,    pre, n, parts) {
		if (index(line, "(") > 0)
			pre = substr(line, 1, index(line, "(") - 1)
		else {
			pre = line
			sub(/ = .*$/, "", pre)
			sub(/;$/, "", pre)
		}
		n = split(pre, parts, " ")
		return parts[n]
	}
	/^Compiled from / { next }
	/^[a-z].* \{$/ {
		header = $0
		sub(/ \{$/, "", header)
		visible = (header ~ /^(public|protected) /)
		n = split(header, words, " ")
		name = ""
		for (i = 1; i < n; i++)
			if (words[i] == "class" || words[i] == "interface" || words[i] == "enum") {
				name = words[i + 1]
				break
			}
		sub(/<.*$/, "", name)
		if (visible)
			printf "%s\t0\t%s\n", name, header
		next
	}
	/^}$/ { visible = 0; next }
	/^  / {
		if (!visible)
			next
		line = $0
		sub(/^ +/, "", line)
		m = member_name(line)
		# constructors are named after the class (with dots); keep them all
		if (index(m, ".") == 0 && m !~ /^[^$]+(\$default)?$/)
			next
		printf "%s\t1\t  %s\n", name, line
	}
' "$tmp/javap.txt" \
	| LC_ALL=C sort -t "$(printf '\t')" -k1,1 -k2,2 -k3 \
	| LC_ALL=C awk -F'\t' '
		$1 != last { if (NR > 1) print "}\n"; last = $1; print $3 " {"; next }
		{ print $3 }
		END { if (NR > 0) print "}" }
	' > "$tmp/api.txt"

if [ "$mode" = update ]; then
	mkdir -p "$(dirname "$api_file")"
	cp "$tmp/api.txt" "$api_file"
	echo "Updated ${api_file#"$repo_root"/} ($(grep -c . "$api_file") lines)"
	exit 0
fi

if [ ! -f "$api_file" ]; then
	echo "Error: ${api_file#"$repo_root"/} does not exist; create it with $0 --update" >&2
	exit 1
fi
if ! diff -u --label "${api_file#"$repo_root"/} (checked in)" --label "built from $(basename "$input")" \
	"$api_file" "$tmp/api.txt"; then
	cat >&2 <<EOF

The public API differs from ${api_file#"$repo_root"/}.
- Lines starting with '-' were removed or changed: that breaks apps compiled against
  an earlier release. Restore them unless this is a major release.
- Lines starting with '+' are additions. If they are intended, run
  buildscripts/check-api.sh --update and commit the updated file with the change.
EOF
	exit 1
fi
echo "Public API matches ${api_file#"$repo_root"/}"
