#!/bin/bash
# Prints one Markdown table row of test counts, for the job summaries in build.yaml:
#   | <label> | <tests> | <passed> | <failed> | <skipped> |
#
#   test-summary.sh header                   the table header
#   test-summary.sh junit <label> <dir>...   JUnit XML reports (TEST-*.xml) under the dirs,
#                                            as Gradle writes them for unit and connected tests
#   test-summary.sh cargo <label> <log>      the "test result:" lines of cargo test's output
#
# Exits 1 when no test was found, so a run that silently ran nothing is not reported as a pass
# (the row is printed anyway).
set -euo pipefail

mode=${1:?usage: test-summary.sh header | junit <label> <dir>... | cargo <label> <log>}

if [ "$mode" = header ]; then
	echo "| Tests | ran | passed | failed | skipped |"
	echo "|---|---|---|---|---|"
	exit 0
fi

label=${2:?a label is needed}
shift 2

case "$mode" in
	junit)
		files=()
		for dir in "$@"; do
			[ -d "$dir" ] || continue
			while IFS= read -r f; do files+=("$f"); done < <(find "$dir" -type f -name 'TEST-*.xml' | sort)
		done
		if [ ${#files[@]} -eq 0 ]; then
			counts="0 0 0"
		else
			# the attributes of each file's <testsuite> element (one per file, in Gradle's reports)
			counts=$(awk '
				function attr(name,    s) {
					if (!match(head, " " name "=\"[0-9]+\"")) return 0
					s = substr(head, RSTART, RLENGTH); gsub(/[^0-9]/, "", s); return s + 0
				}
				/<testsuite[ >]/ && !done[FILENAME] {
					head = $0; sub(/^.*<testsuite /, " ", head); sub(/>.*$/, "", head)
					t += attr("tests"); f += attr("failures") + attr("errors"); s += attr("skipped")
					done[FILENAME] = 1
				}
				END { print t + 0, f + 0, s + 0 }' "${files[@]}")
		fi
		;;
	cargo)
		log=${1:?a cargo test log is needed}
		# test result: ok. 65 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; ...
		counts=$(awk '
			/^test result: / {
				for (i = 1; i <= NF; i++) {
					n = $(i - 1) + 0
					if ($i ~ /^passed;?$/) p += n
					else if ($i ~ /^failed;?$/) f += n
					else if ($i ~ /^ignored;?$/) s += n
				}
			}
			END { print p + f + s, f + 0, s + 0 }' "$log")
		;;
	*)
		echo "test-summary.sh: unknown mode $mode" >&2
		exit 2
		;;
esac

read -r total failed skipped <<< "$counts"
passed=$((total - failed - skipped))
echo "| $label | $total | $passed | $failed | $skipped |"
[ "$total" -gt 0 ] || { echo "test-summary.sh: no tests found for $label" >&2; exit 1; }
