#!/bin/bash -e
# Prints one line per dependency in deps/: a SHA-256 over every file and symlink of its
# tree (outside .git), then its name. Run it right after patch.sh, before building:
# some builds (lua, mbedtls, libass) write into the source tree.
#
# CI uses it to prove that the corresponding-source tarball holds exactly the sources
# each ABI was built from (the ABI jobs and the source job download separately).
#
# Usage (from buildscripts/): ./include/hash-sources.sh > sources.sha256

[ -d deps ] || { echo "hash-sources.sh: no deps/ here, run it from buildscripts/" >&2; exit 2; }
cd deps

export LC_ALL=C
for dep in */; do
	dep=${dep%/}
	hash=$(
		cd "$dep"
		{
			find . -name .git -prune -o -type f -print0 | sort -z | xargs -0 -r sha256sum
			find . -name .git -prune -o -type l -printf 'link %p -> %l\n' | sort
		} | sha256sum | cut -d' ' -f1
	)
	echo "$hash  $dep"
done
