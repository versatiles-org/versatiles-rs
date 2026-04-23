#!/usr/bin/env bash

PROJECT_DIR="$(dirname "$0")/.."

SORT_MODE="path"
CARGO_ARGS=()

usage() {
	cat <<'EOF'
Usage: test-coverage.sh [OPTIONS] [CARGO_ARGS...]

Run 'cargo llvm-cov test' across the workspace, then print a sorted
per-file coverage summary.

Sort options (default: --by-path):
  --by-path       Sort rows by file path ascending
  --by-missed     Sort rows by missed lines descending
  --by-coverage   Sort rows by coverage percentage descending
  -h, --help      Show this help

Any other arguments are forwarded to 'cargo llvm-cov test'.
EOF
}

while [[ $# -gt 0 ]]; do
	case "$1" in
		--by-path)     SORT_MODE="path"; shift ;;
		--by-missed)   SORT_MODE="missed"; shift ;;
		--by-coverage) SORT_MODE="coverage"; shift ;;
		-h|--help)     usage; exit 0 ;;
		*)             CARGO_ARGS+=("$1"); shift ;;
	esac
done

# Skip e2e tests (test functions prefixed with e2e_) during coverage.
# RUST_LOG=off silences env_logger output from tests; --quiet (both for cargo
# and the test binary) suppresses compilation progress and per-test lines.
RUST_LOG=off cargo llvm-cov test --quiet --workspace --all-features --tests --lcov \
	--output-path "$PROJECT_DIR/lcov.info" "${CARGO_ARGS[@]}" -- --skip e2e_

cargo llvm-cov report --color always | awk '
{
   if (NR == 1) {
      end1   = index($0, "Regions")  - 1
      start2 = index($0, " Lines")   + 1
      end2   = index($0, "Branches") - 1
      offset1 = 0
      offset2 = 0
   }
   if (NR == 3) {
      offset1 = 33
      offset2 = 18
   }
   print substr($0, 1, end1) substr($0, start2 + offset1, end2 - start2 + 1 + offset2)
}' | awk -v mode="$SORT_MODE" '
function sort_key(line,   clean, n, f, val) {
	clean = line
	gsub(/\033\[[0-9;]*m/, "", clean)
	n = split(clean, f, /[ \t]+/)
	if (mode == "missed") {
		# Descending missed lines: invert via large constant, then asc. sort.
		return sprintf("%09d", 999999999 - (f[n-1] + 0))
	}
	if (mode == "coverage") {
		val = f[n]; sub(/%/, "", val)
		# Descending coverage percent (4 decimals of precision).
		return sprintf("%09d", 10000000 - int((val + 0) * 10000))
	}
	return f[1]
}

/^-+$/      { section++; sep[section] = $0; next }
section == 0 { hdr[++hn] = $0; next }
section == 1 { body[++bn] = $0; next }
section == 2 { ftr[++fn] = $0; next }

END {
	for (i = 1; i <= hn; i++) print hdr[i]
	print sep[1]
	# LC_ALL=C forces byte-wise ASCII collation so "/" (47) sorts before "_" (95),
	# which matches the raw cargo llvm-cov ordering for "versatiles/" vs "versatiles_*".
	pipe = "LC_ALL=C sort | cut -f2-"
	for (i = 1; i <= bn; i++) {
		print sort_key(body[i]) "\t" body[i] | pipe
	}
	close(pipe)
	print sep[2]
	for (i = 1; i <= fn; i++) print ftr[i]
}'
