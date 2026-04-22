#!/usr/bin/env bash
# Measure how long it takes to open each container reader in two contexts:
#
#   - server     : just open the source (no pyramid scan)
#                  → uses `versatiles dev print-tilejson` which only reads metadata.
#
#   - conversion : open the source AND compute the tile pyramid
#                  → uses `versatiles dev count-tiles --level=0` which forces
#                    `TileSource::tile_pyramid().await` and then enumerates a
#                    single (level-0) bbox — the latter is negligible compared
#                    to the pyramid scan.
#
# Each measurement is the minimum of $ITERATIONS warm-cache runs. Cold-cache
# runs (i.e. after evicting the OS page cache) are out of scope here.
#
# Usage:
#   ./scripts/bench-init.sh [ITERATIONS]
#   ITERATIONS=10 ./scripts/bench-init.sh
#
# Example output:
#   container       server (ms)   conversion (ms)
#   ──────────────────────────────────────────────
#   mbtiles                  31              162
#   pmtiles                  29              154
#   versatiles               28               29
#   directory                47               48
#   tar                      62               63

set -uo pipefail
cd "$(dirname "$0")/.."

ITERATIONS=${1:-${ITERATIONS:-5}}
TESTDATA="$(pwd)/testdata"
SOURCE_VERSATILES="$TESTDATA/berlin.versatiles"

RED=$'\033[0;31m'; YELLOW=$'\033[0;33m'; GREEN=$'\033[0;32m'
CYAN=$'\033[0;36m'; BOLD=$'\033[1m'; RESET=$'\033[0m'

# ── 1. Build release binary ──────────────────────────────────────────────────
echo "${BOLD}${CYAN}Building release binary (versatiles --features cli)…${RESET}" >&2
cargo build --release -p versatiles --features cli >&2
VERSATILES="$(pwd)/target/release/versatiles"

# ── 2. Make sure all 5 container types exist as test data ────────────────────
ensure_test_data() {
  local target=$1
  if [[ -e "$target" ]]; then return; fi
  echo "${YELLOW}Creating $(basename "$target") from berlin.versatiles…${RESET}" >&2
  "$VERSATILES" convert "$SOURCE_VERSATILES" "$target" >&2
}

ensure_test_data_directory() {
  local target=$1
  if [[ -d "$target" ]]; then return; fi
  echo "${YELLOW}Creating $(basename "$target")/ from berlin.versatiles…${RESET}" >&2
  mkdir -p "$target"
  "$VERSATILES" convert "$SOURCE_VERSATILES" "$target" >&2
}

ensure_test_data "$TESTDATA/berlin.tar"
ensure_test_data_directory "$TESTDATA/berlin.directory"
# mbtiles, pmtiles, versatiles are already in the repo.

# ── 3. Measure: best (min) of N iterations, returns milliseconds ─────────────
# Uses bash's built-in `time` with %R format → "0.023" (seconds).
# Includes binary startup, but startup is identical across cases, so
# relative comparisons remain meaningful.
measure_ms() {
  local input=$1; shift
  local best=99999.0
  for _ in $(seq 1 "$ITERATIONS"); do
    local TIMEFORMAT='%R'
    local s
    s=$( { time "$VERSATILES" "$@" "$input" > /dev/null 2>&1; } 2>&1 | tr -d ' ' )
    # Compare as floats; keep the smaller value.
    if awk -v a="$s" -v b="$best" 'BEGIN{exit !(a<b)}'; then
      best="$s"
    fi
  done
  awk -v s="$best" 'BEGIN{printf "%.0f", s*1000}'
}

# ── 4. Measure baseline binary startup so we can subtract it ─────────────────
# `versatiles --version` runs no I/O and no opens; what's left is binary launch
# + clap parsing. We subtract this from each measurement to isolate the actual
# open / open+pyramid cost.
measure_startup_ms() {
  local best=99999.0
  for _ in $(seq 1 "$ITERATIONS"); do
    local TIMEFORMAT='%R'
    local s
    s=$( { time "$VERSATILES" --version > /dev/null 2>&1; } 2>&1 | tr -d ' ' )
    if awk -v a="$s" -v b="$best" 'BEGIN{exit !(a<b)}'; then
      best="$s"
    fi
  done
  awk -v s="$best" 'BEGIN{printf "%.0f", s*1000}'
}

STARTUP_MS=$(measure_startup_ms)

# ── 5. Run measurements and print the table ──────────────────────────────────
echo
printf "${BOLD}%-12s  %12s  %16s  %12s${RESET}\n" \
  "container" "server (ms)" "conversion (ms)" "Δ (ms)"
printf '%.0s─' $(seq 1 60); echo

for entry in \
  "mbtiles    :$TESTDATA/berlin.mbtiles" \
  "pmtiles    :$TESTDATA/berlin.pmtiles" \
  "versatiles :$TESTDATA/berlin.versatiles" \
  "directory  :$TESTDATA/berlin.directory" \
  "tar        :$TESTDATA/berlin.tar"
do
  name=${entry%%:*}
  path=${entry#*:}
  server_raw=$(measure_ms "$path" dev print-tilejson)
  conv_raw=$(measure_ms   "$path" dev count-tiles --level=0)

  # Subtract baseline; clamp to >= 0
  server_net=$(awk -v r="$server_raw" -v b="$STARTUP_MS" 'BEGIN{v=r-b; if (v<0) v=0; printf "%.0f", v}')
  conv_net=$(awk   -v r="$conv_raw"   -v b="$STARTUP_MS" 'BEGIN{v=r-b; if (v<0) v=0; printf "%.0f", v}')
  delta=$(awk      -v c="$conv_net"   -v s="$server_net" 'BEGIN{printf "%+d", c-s}')

  # Colour the Δ column to highlight readers that pay a big lazy-pyramid cost.
  ratio_num=$(awk -v c="$conv_net" -v s="$server_net" 'BEGIN{ if (s>0) printf "%.2f", c/s; else if (c>0) print "9.99"; else print "1.00" }')
  if   awk -v r="$ratio_num" 'BEGIN{exit !(r>=2.0)}'; then color="$RED"
  elif awk -v r="$ratio_num" 'BEGIN{exit !(r>=1.3)}'; then color="$YELLOW"
  else                                                     color="$GREEN"; fi

  printf "%-12s  %12s  %16s  %s%12s%s\n" \
    "$name" "$server_net" "$conv_net" "$color" "$delta" "$RESET"
done

echo
echo "  baseline binary startup subtracted = ${BOLD}${STARTUP_MS} ms${RESET} (best of $ITERATIONS runs of \`versatiles --version\`)"
echo "  ${BOLD}server${RESET}     = open only (TileSource::tile_pyramid is left lazy)"
echo "  ${BOLD}conversion${RESET} = open + force tile_pyramid().await"
echo "  ${BOLD}Δ${RESET}          = conversion − server (cost of pyramid computation)"
echo "  best of $ITERATIONS runs each, warm cache"
