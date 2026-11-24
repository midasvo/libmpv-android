#!/bin/bash
# Checks that README.md says what the code says, in two places:
#
# 1. The licence table says what the SBOM says: the same components, each with the same
#    licence as in sbom.sh. Versions are not compared, and the README does not list them,
#    so a dependency bump (Renovate edits only depinfo.sh) never fails this check. Only
#    adding, removing or relicensing a component does, and then the README has to be
#    updated with sbom.sh.
#
#    The table sits between <!-- licence-table --> and <!-- /licence-table -->. Each row:
#    | <name> [(note)] | `<licence>` |  where <name> is the component's name in the SBOM
#    (case does not matter) and <licence> is exactly its SPDX expression, or the free text
#    sbom.sh gives where there is no SPDX identifier.
#
# 2. Every Kotlin example is code that compiles. The examples live in
#    libmpv/src/test/java/dev/jdtech/mpv/docs/DocExamples.kt, compiled with the unit tests,
#    as regions `// example:<name>` ... `// end:<name>`; the imports a reader sees are the
#    region `<name>-imports`, commented out with "// ". In the README, the line right before
#    a ```kotlin block (blank lines aside) must be one of:
#      <!-- example:<name> -->            the block must equal the imports region (without
#                                         the "// "), a blank line, and the region (without
#                                         its indentation); a region with no imports is the
#                                         block on its own
#      <!-- example-unchecked: <why> -->  not checked, on purpose (a Gradle script, say)
#    A ```kotlin block without either, a marker without a block, and a marker naming a
#    region that does not exist all fail, so a new example cannot skip the check.
#
# Usage: check-docs.sh [path-to-README] [path-to-DocExamples.kt]
#   (defaults: README.md at the repository root, and the file above)
# Exit status: 0 when both match, 1 when something differs, 2 when a file or the licence
# table is missing.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$here/.." && pwd)
readme=${1:-$repo_root/README.md}
examples=${2:-$repo_root/libmpv/src/test/java/dev/jdtech/mpv/docs/DocExamples.kt}

for f in "$readme" "$examples"; do
	[ -f "$f" ] || { echo "check-docs.sh: $f not found" >&2; exit 2; }
done

grep -q '^<!-- licence-table -->$' "$readme" && grep -q '^<!-- /licence-table -->$' "$readme" || {
	echo "check-docs.sh: no <!-- licence-table --> ... <!-- /licence-table --> block in $readme" >&2
	exit 2
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

licence_problems=()
example_problems=()

# ---------------------------------------------------------------- 1. licences

# names are compared without case (the README writes FFmpeg, the SBOM ffmpeg), licences exactly
lower_name () { awk -F'\t' -v OFS='\t' '{ $1 = tolower($1); print }'; }

# name<TAB>licence, from the SBOM
expected=$("$here/site-data.sh" --list | awk -F'\t' '{ print $1 "\t" $3 }' | lower_name | sort)

# name<TAB>licence, from the README rows (header and separator rows have no backticks)
actual=$(awk '
	/^<!-- licence-table -->$/ { inside = 1; next }
	/^<!-- \/licence-table -->$/ { inside = 0 }
	inside && /^\|/ {
		n = split($0, cell, "|")
		if (n < 4) next
		name = cell[2]; lic = cell[3]
		if (lic !~ /`/) next
		sub(/\(.*\)/, "", name)
		gsub(/^[ \t]+|[ \t]+$/, "", name)
		gsub(/`/, "", name)
		sub(/^[^`]*`/, "", lic); sub(/`.*$/, "", lic)
		print name "\t" lic
	}' "$readme" | lower_name | sort)

if [ -z "$actual" ]; then
	echo "check-docs.sh: the licence table in $readme has no rows" >&2
	exit 2
fi

while IFS=$'\t' read -r name licence; do
	readme_licence=$(awk -F'\t' -v n="$name" '$1 == n { print $2; exit }' <<< "$actual")
	if [ -z "$readme_licence" ] && ! awk -F'\t' -v n="$name" '$1 == n { f = 1 } END { exit !f }' <<< "$actual"; then
		licence_problems+=("$name is in the SBOM ($licence) but not in the README table")
	elif [ "$readme_licence" != "$licence" ]; then
		licence_problems+=("$name: README says '$readme_licence', sbom.sh says '$licence'")
	fi
done <<< "$expected"
while IFS=$'\t' read -r name _; do
	awk -F'\t' -v n="$name" '$1 == n { f = 1 } END { exit !f }' <<< "$expected" ||
		licence_problems+=("$name is in the README table but not in the SBOM")
done <<< "$actual"

# ---------------------------------------------------------------- 2. examples

mkdir -p "$tmp/region" "$tmp/readme"

# Every region of DocExamples.kt into region/<name>, as the reader sees it: the common
# indentation removed, and in an -imports region the leading "// " of each line.
tr -d '\r' < "$examples" | awk -v dir="$tmp/region" '
	function flush(   i, line, min, n, ind) {
		min = -1
		for (i = 1; i <= count; i++) {
			line = lines[i]
			if (line ~ /^[ \t]*$/) continue
			match(line, /^[ \t]*/)
			if (min < 0 || RLENGTH < min) min = RLENGTH
		}
		if (min < 0) min = 0
		printf "" > (dir "/" name)
		for (i = 1; i <= count; i++) {
			line = lines[i]
			if (line ~ /^[ \t]*$/) line = ""
			else line = substr(line, min + 1)
			if (name ~ /-imports$/) sub(/^\/\/ ?/, "", line)
			print line > (dir "/" name)
		}
		close(dir "/" name)
	}
	/^[ \t]*\/\/ example:[A-Za-z0-9_-]+[ \t]*$/ {
		if (name != "") { print "nested " $0 > "/dev/stderr"; exit 3 }
		name = $0; sub(/^[ \t]*\/\/ example:/, "", name); sub(/[ \t]*$/, "", name)
		count = 0; next
	}
	/^[ \t]*\/\/ end:[A-Za-z0-9_-]+[ \t]*$/ {
		end = $0; sub(/^[ \t]*\/\/ end:/, "", end); sub(/[ \t]*$/, "", end)
		if (end != name) { print "end:" end " closes example:" name > "/dev/stderr"; exit 3 }
		flush(); name = ""; next
	}
	name != "" { lines[++count] = $0 }
	END { if (name != "") { print "example:" name " has no end" > "/dev/stderr"; exit 3 } }
' || { echo "check-docs.sh: the regions in $examples are malformed (see above)" >&2; exit 2; }

# The README's ```kotlin blocks: each marked one into readme/<name>, and one line per
# block or marker on stdout: "example <name> <line>", "unchecked <line> <reason>",
# "unmarked <line>" or "orphan <line> <marker>".
tr -d '\r' < "$readme" | awk -v dir="$tmp/readme" '
	function marker_of(s) {
		if (s ~ /^<!-- example:[A-Za-z0-9_-]+ -->$/) { sub(/^<!-- example:/, "", s); sub(/ -->$/, "", s); return "example " s }
		if (s ~ /^<!-- example-unchecked: .+ -->$/) { sub(/^<!-- example-unchecked: /, "", s); sub(/ -->$/, "", s); return "unchecked " s }
		return ""
	}
	fence && /^```[ \t]*$/ {
		fence = 0
		if (out != "") close(out)
		out = ""
		next
	}
	fence { if (out != "") print > out; next }
	/^```/ {
		fence = 1; out = ""
		if ($0 ~ /^```(kotlin|kt)[ \t]*$/) {
			if (pending ~ /^example /) {
				name = substr(pending, 9)
				out = dir "/" name
				if (name in seen) print "duplicate " NR " " name
				seen[name] = 1
				printf "" > out
				print "example " name " " NR
			} else if (pending ~ /^unchecked /) {
				print "unchecked " NR " " substr(pending, 11)
			} else {
				print "unmarked " NR
			}
		} else if (pending != "") {
			print "orphan " pending_line " " pending_raw
		}
		pending = ""
		next
	}
	/^[ \t]*$/ { next }
	{
		if (pending != "") print "orphan " pending_line " " pending_raw
		pending = marker_of($0); pending_line = NR; pending_raw = $0
	}
	END { if (pending != "") print "orphan " pending_line " " pending_raw }
' > "$tmp/blocks.txt"

readme_name=${readme#"$repo_root"/}
examples_name=${examples#"$repo_root"/}
checked=0 unchecked=0
while read -r kind a b; do
	case "$kind" in
		example)
			name=$a line=$b
			if [ ! -f "$tmp/region/$name" ]; then
				example_problems+=("$readme_name:$line: <!-- example:$name --> has no region // example:$name in $examples_name")
				continue
			fi
			want="$tmp/want-$name"
			if [ -s "$tmp/region/$name-imports" ]; then
				{ cat "$tmp/region/$name-imports"; echo; cat "$tmp/region/$name"; } > "$want"
			else
				cat "$tmp/region/$name" > "$want"
			fi
			if ! diff -u --label "$examples_name (example:$name)" --label "$readme_name:$line" \
					"$want" "$tmp/readme/$name" > "$tmp/diff-$name"; then
				example_problems+=("$readme_name:$line: the example differs from its region // example:$name:
$(cat "$tmp/diff-$name")")
			fi
			checked=$((checked + 1))
			;;
		unchecked) unchecked=$((unchecked + 1)) ;;
		unmarked)
			example_problems+=("$readme_name:$a: a \`\`\`kotlin block without <!-- example:<name> --> (or <!-- example-unchecked: <why> -->) on the line before it")
			;;
		orphan) example_problems+=("$readme_name:$a: $b is not followed by a \`\`\`kotlin block") ;;
		duplicate) example_problems+=("$readme_name:$a: example:$b is shown twice") ;;
	esac
done < "$tmp/blocks.txt"

# A region that no README block shows is not an error (the site, say, may use it), but say so
for f in "$tmp/region"/*; do
	[ -e "$f" ] || continue
	name=${f##*/}
	case "$name" in *-imports) continue ;; esac
	grep -q "^example $name " "$tmp/blocks.txt" || echo "check-docs.sh: note: region example:$name is not shown in $readme_name"
done

# ---------------------------------------------------------------- result

status=0
if [ ${#licence_problems[@]} -gt 0 ]; then
	echo "Licence table in $readme does not match buildscripts/sbom.sh:" >&2
	printf '  - %s\n' "${licence_problems[@]}" >&2
	echo "Fix the README (or sbom.sh, if the README is right) so that both say the same." >&2
	status=1
else
	echo "README licence table matches sbom.sh ($(wc -l <<< "$expected") components)"
fi

if [ ${#example_problems[@]} -gt 0 ]; then
	echo "Kotlin examples in $readme do not match $examples_name:" >&2
	printf -- '- %s\n' "${example_problems[@]}" >&2
	echo "Change the region in $examples_name (it is compiled by :libmpv:testDebugUnitTest) and copy it into the README, or the other way round." >&2
	status=1
else
	echo "README Kotlin examples match $examples_name ($checked checked, $unchecked marked unchecked)"
fi
exit $status
