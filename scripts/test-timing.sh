#!/usr/bin/env bash
# Measure and analyse test runtimes to identify slow tests.
#
# Usage:
#   ./scripts/test-timing.sh [cargo-test-args]
#
# Examples:
#   ./scripts/test-timing.sh
#   ./scripts/test-timing.sh --package versatiles_pipeline
#   ./scripts/test-timing.sh -- my_specific_test

set -uo pipefail
cd "$(dirname "$0")/.."

TOP_N=30         # how many slowest tests to show in the ranked list
SLOW_MS=100      # threshold (ms) to mark a test as slow in the module summary
MEDIUM_MS=10     # threshold (ms) to mark a test as medium

# ── colour helpers ────────────────────────────────────────────────────────────
RED=$'\033[0;31m'; YELLOW=$'\033[0;33m'; GREEN=$'\033[0;32m'
CYAN=$'\033[0;36m'; BOLD=$'\033[1m'; RESET=$'\033[0m'

echo "${BOLD}${CYAN}Building and running tests with timing…${RESET}"
echo

# ── run tests, capture stdout+stderr ─────────────────────────────────────────
RAW=$(cargo +nightly test --lib --bins --all-features --workspace \
  -- -Zunstable-options --report-time --skip e2e_ "$@" 2>&1 || true)

# ── extract lines: "test <name> ... ok|FAILED <Xs>" ──────────────────────────
# Timing format from libtest: <0.123s> or <12.345s>
TIMING_LINES=$(echo "$RAW" | grep -E '^test .+ \.\.\. (ok|FAILED) <[0-9]+\.[0-9]+s>$' || true)

if [[ -z "$TIMING_LINES" ]]; then
  echo "${RED}No timing data found. Is the nightly toolchain installed?${RESET}"
  echo "Install it with: rustup toolchain install nightly"
  exit 1
fi

# ── parse into a TSV: ms<TAB>status<TAB>full_test_name ───────────────────────
PARSED=$(echo "$TIMING_LINES" | awk '{
  # line: "test <name> ... ok <Xs>"
  name = $2
  status = $4          # "ok" or "FAILED"
  time_str = $5        # "<0.123s>"
  gsub(/[<>s]/, "", time_str)
  ms = int(time_str * 1000 + 0.5)
  print ms "\t" status "\t" name
}')

TOTAL_TESTS=$(echo "$PARSED" | wc -l | tr -d ' ')
FAILED_TESTS=$(echo "$PARSED" | awk -F'\t' '$2=="FAILED"' | wc -l | tr -d ' ')
TOTAL_MS=$(echo "$PARSED" | awk -F'\t' '{s+=$1} END{print s}')

# ── ranked list of slowest tests ─────────────────────────────────────────────
echo "${BOLD}Top ${TOP_N} slowest tests${RESET}"
echo "$(printf '%.0s─' {1..72})"
printf "${BOLD}%-8s  %-8s  %s${RESET}\n" "Time" "Status" "Test"
echo "$(printf '%.0s─' {1..72})"

echo "$PARSED" | sort -t$'\t' -k1 -rn | head -n "$TOP_N" | awk -F'\t' \
  -v red="$RED" -v yellow="$YELLOW" -v green="$GREEN" \
  -v reset="$RESET" -v slow="$SLOW_MS" -v medium="$MEDIUM_MS" '
{
  ms=$1; status=$2; name=$3
  if (ms >= slow)        colour = red
  else if (ms >= medium) colour = yellow
  else                   colour = green

  status_colour = (status == "ok") ? green : red

  if (ms >= 1000) {
    time_fmt = sprintf("%.2fs  ", ms/1000)
  } else {
    time_fmt = sprintf("%dms    ", ms)
  }
  printf "%s%-8s%s  %s%-8s%s  %s\n", colour, time_fmt, reset, status_colour, status, reset, name
}'

echo

# ── per-module summary ────────────────────────────────────────────────────────
echo "${BOLD}Time by module (crate::module)${RESET}"
echo "$(printf '%.0s─' {1..72})"
printf "${BOLD}%-8s  %-5s  %-5s  %s${RESET}\n" "Total" "Tests" "Slow" "Module"
echo "$(printf '%.0s─' {1..72})"

echo "$PARSED" | awk -F'\t' -v slow="$SLOW_MS" '
{
  ms=$1; name=$3
  # derive module: drop last "::<test_name>" segment
  n = split(name, parts, "::")
  module = parts[1]
  for (i=2; i<=n-1; i++) module = module "::" parts[i]
  if (n == 1) module = name   # top-level

  total[module]  += ms
  count[module]  += 1
  if (ms >= slow) slow_count[module] += 1
}
END {
  for (m in total) print total[m] "\t" count[m] "\t" (slow_count[m]+0) "\t" m
}' | sort -t$'\t' -k1 -rn | head -n "$TOP_N" | awk -F'\t' \
  -v red="$RED" -v yellow="$YELLOW" -v green="$GREEN" -v bold="$BOLD" \
  -v reset="$RESET" -v slow_thresh="$SLOW_MS" '
{
  ms=$1; cnt=$2; slow=$3; mod=$4
  time_fmt = sprintf("%.2fs  ", ms/1000)
  colour = (slow > 0) ? red : (ms >= slow_thresh ? yellow : green)
  printf "%s%-9s%s  %-5d  %-5d  %s\n", colour, time_fmt, reset, cnt, slow, mod
}'

echo

# ── overall summary ───────────────────────────────────────────────────────────
echo "${BOLD}Summary${RESET}"
echo "$(printf '%.0s─' {1..72})"

if [[ $TOTAL_MS -ge 1000 ]]; then
  TOTAL_FMT=$(awk "BEGIN{printf \"%.2fs\", $TOTAL_MS/1000}")
else
  TOTAL_FMT="${TOTAL_MS}ms"
fi

MEAN_MS=$(awk "BEGIN{printf \"%d\", $TOTAL_MS/$TOTAL_TESTS}")
SLOW_COUNT=$(echo "$PARSED" | awk -F'\t' -v t="$SLOW_MS" '$1>=t' | wc -l | tr -d ' ')

echo "  Total tests  : ${BOLD}${TOTAL_TESTS}${RESET}"
if [[ "$FAILED_TESTS" -gt 0 ]]; then
  echo "  Failed       : ${RED}${BOLD}${FAILED_TESTS}${RESET}"
fi
echo "  Wall time    : ${BOLD}${TOTAL_FMT}${RESET}  (sum of all test durations)"
echo "  Mean / test  : ${MEAN_MS}ms"
echo "  Slow (≥${SLOW_MS}ms): $([ "$SLOW_COUNT" -gt 0 ] && echo "${RED}" || echo "${GREEN}")${SLOW_COUNT}${RESET}"
echo
echo "  Thresholds   : ${RED}slow ≥ ${SLOW_MS}ms${RESET}  ${YELLOW}medium ≥ ${MEDIUM_MS}ms${RESET}  ${GREEN}fast < ${MEDIUM_MS}ms${RESET}"
