#!/bin/bash
# Prints, as Markdown for the release notes, what changed in the public API
# (libmpv/api/libmpv.api, see check-api.sh) between a previous release tag and this
# checkout: the lines added and removed, counted and listed, each member with its class.
# Removed lines break apps compiled against the previous release.
#
# Usage: api-diff.sh <previous-tag> [current-dump]   (default: libmpv/api/libmpv.api; any
# commit works in place of the tag)
# A tag without the file (1.3.0 and earlier, before the dump existed) is reported as such.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
path=libmpv/api/libmpv.api
prev=${1:?usage: api-diff.sh <previous-tag> [current-dump]}
current=${2:-$repo_root/$path}

[ -f "$current" ] || { echo "api-diff.sh: $current not found" >&2; exit 2; }
git -C "$repo_root" rev-parse -q --verify "$prev^{commit}" > /dev/null ||
	{ echo "api-diff.sh: no tag or commit $prev in this clone (git fetch --tags)" >&2; exit 2; }

if ! git -C "$repo_root" cat-file -e "$prev:$path" 2>/dev/null; then
	echo "**Public API:** $prev has no \`$path\` (the API dump was added after it), so there is nothing to compare with; this release's API is the $(grep -c '^  ' "$current") members of $(grep -c ' {$' "$current") classes in that file."
	exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
git -C "$repo_root" show "$prev:$path" | tr -d '\r' > "$tmp/old.api"
tr -d '\r' < "$current" > "$tmp/new.api"

# One line per class header and per member, the member prefixed with its class, so that a
# line names what it is about on its own: "public final class X" and "X:  public void m();"
flatten () {
	LC_ALL=C awk '
		/ \{$/ { cls = $0; sub(/ \{$/, "", cls); print cls; next }
		/^}$/ || /^$/ { next }
		/^  / { m = $0; sub(/^ +/, "", m); name = cls; sub(/^.*(class|interface|enum) /, "", name); sub(/ .*$/, "", name); print name ": " m }
	' "$1" | LC_ALL=C sort -u
}
flatten "$tmp/old.api" > "$tmp/old.txt"
flatten "$tmp/new.api" > "$tmp/new.txt"
LC_ALL=C comm -13 "$tmp/old.txt" "$tmp/new.txt" > "$tmp/added.txt"
LC_ALL=C comm -23 "$tmp/old.txt" "$tmp/new.txt" > "$tmp/removed.txt"
added=$(grep -c . "$tmp/added.txt" || true)
removed=$(grep -c . "$tmp/removed.txt" || true)

if [ "$added" -eq 0 ] && [ "$removed" -eq 0 ]; then
	echo "**Public API:** unchanged since $prev (\`$path\`)."
	exit 0
fi

echo "**Public API** since $prev (\`$path\`): $added added, $removed removed or changed$([ "$removed" -gt 0 ] && echo " (**breaking**: apps compiled against $prev may fail to link)")."
for kind in removed added; do
	[ -s "$tmp/$kind.txt" ] || continue
	sign=+; [ "$kind" = removed ] && sign=-
	echo
	echo "<details><summary>$(grep -c . "$tmp/$kind.txt") $kind</summary>"
	echo
	echo '```diff'
	sed "s/^/$sign /" "$tmp/$kind.txt"
	echo '```'
	echo
	echo "</details>"
done
