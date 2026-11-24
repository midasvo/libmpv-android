#!/bin/bash
# Checks that the licence table in README.md says what the SBOM says: the same
# components, each with the same licence as in sbom.sh. Versions are not compared, and
# the README does not list them, so a dependency bump (Renovate edits only depinfo.sh)
# never fails this check. Only adding, removing or relicensing a component does, and
# then the README has to be updated with sbom.sh.
#
# The table sits between <!-- licence-table --> and <!-- /licence-table --> in the
# README. Each row: | <name> [(note)] | `<licence>` |  where <name> is the component's
# name in the SBOM (case does not matter) and <licence> is exactly its SPDX expression, or the
# free text sbom.sh gives where there is no SPDX identifier.
#
# Usage: check-docs.sh [path-to-README]   (default: README.md at the repository root)
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readme=${1:-$here/../README.md}

[ -f "$readme" ] || { echo "check-docs.sh: $readme not found" >&2; exit 2; }

grep -q '^<!-- licence-table -->$' "$readme" && grep -q '^<!-- /licence-table -->$' "$readme" || {
	echo "check-docs.sh: no <!-- licence-table --> ... <!-- /licence-table --> block in $readme" >&2
	exit 2
}

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

problems=()
while IFS=$'\t' read -r name licence; do
	readme_licence=$(awk -F'\t' -v n="$name" '$1 == n { print $2; exit }' <<< "$actual")
	if [ -z "$readme_licence" ] && ! awk -F'\t' -v n="$name" '$1 == n { f = 1 } END { exit !f }' <<< "$actual"; then
		problems+=("$name is in the SBOM ($licence) but not in the README table")
	elif [ "$readme_licence" != "$licence" ]; then
		problems+=("$name: README says '$readme_licence', sbom.sh says '$licence'")
	fi
done <<< "$expected"
while IFS=$'\t' read -r name _; do
	awk -F'\t' -v n="$name" '$1 == n { f = 1 } END { exit !f }' <<< "$expected" ||
		problems+=("$name is in the README table but not in the SBOM")
done <<< "$actual"

if [ ${#problems[@]} -gt 0 ]; then
	echo "Licence table in $readme does not match buildscripts/sbom.sh:" >&2
	printf '  - %s\n' "${problems[@]}" >&2
	echo "Fix the README (or sbom.sh, if the README is right) so that both say the same." >&2
	exit 1
fi

echo "README licence table matches sbom.sh ($(wc -l <<< "$expected") components)"
