#!/usr/bin/env bash
# Measure how `raster_mask` scales with the complexity of its GeoJSON.
#
# The mask does three quite different things depending on where a tile falls,
# and only one of them touches pixels:
#
#   - fully inside  : the tile passes through untouched
#   - fully outside : the tile is dropped
#   - partial       : an alpha value is computed for every pixel
#
# So the three are measured separately. A single average over a whole region
# hides which one is expensive, and it is the partial case that carries the
# per-pixel point-in-polygon and distance queries.
#
# Each case is measured against two masks — testdata/borders.geojson at 16
# vertices and testdata/germany.geojson.br at 84143 — so the numbers show how
# the cost scales with vertex count rather than just how long one run took.
#
# Zoom matters independently: a pixel is roughly 10 km across at z4 and 40 m at
# z12, while the mask's distance thresholds are in metres and do not adapt.
#
# Usage:
#   ./scripts/bench-raster-mask.sh [ZOOM...]      # defaults to 6 8 10

set -uo pipefail
cd "$(dirname "$0")/.."

ZOOMS=${*:-"6 8 10"}
BENCHDIR="testdata/bench"
BIG="$BENCHDIR/germany.geojson"
SMALL="testdata/borders.geojson"
BIN="./target/release/versatiles"

mkdir -p "$BENCHDIR"

# The fixture is committed brotli-compressed (210 KB against 1.7 MB raw), and
# raster_mask reads a plain file. Prefer the brotli CLI; fall back to node,
# which the repository already requires and whose zlib does brotli natively.
if [ ! -f "$BIG" ]; then
	echo "decompressing testdata/germany.geojson.br"
	if command -v brotli >/dev/null 2>&1; then
		brotli -dc testdata/germany.geojson.br >"$BIG"
	elif command -v node >/dev/null 2>&1; then
		node -e "const z=require('zlib'),f=require('fs');f.writeFileSync('$BIG',z.brotliDecompressSync(f.readFileSync('testdata/germany.geojson.br')))"
	else
		echo "need either 'brotli' or 'node' to unpack the fixture" >&2
		exit 1
	fi
fi

if [ ! -x "$BIN" ]; then
	echo "building release binary"
	cargo build --release --bin versatiles || exit 1
fi

# Bounding boxes chosen against the German border: one well inside, one far out
# in the Atlantic, one straddling the western border so every tile is partial.
run() { # name geojson bbox zoom
	local out start end
	out=$(mktemp -d)
	start=$(python3 -c 'import time;print(time.time())')
	"$BIN" convert --bbox "$3" --min-zoom "$4" --max-zoom "$4" \
		"[,vpl](from_debug format=png | raster_mask geojson=\"$2\")" \
		"$out/o.versatiles" >/dev/null 2>&1
	end=$(python3 -c 'import time;print(time.time())')
	rm -rf "$out"
	# Tiles fed to the mask, derived from the bbox rather than read back from the
	# output: the mask drops tiles that fall outside, and the cost we care about
	# is per tile examined, not per tile written.
	python3 -c "
import math, time
lon0, lat0, lon1, lat1 = [float(v) for v in '$3'.split(',')]
z = $4
def xt(lon): return int((lon + 180) / 360 * 2**z)
def yt(lat):
    r = math.radians(lat)
    return int((1 - math.log(math.tan(r) + 1/math.cos(r)) / math.pi) / 2 * 2**z)
n = (xt(lon1) - xt(lon0) + 1) * (yt(lat0) - yt(lat1) + 1)
t = $end - $start
print(f'  {\"$1\":<28} z$4  {t:6.2f}s  {n:>6} tiles  {t/n*1000:7.2f} ms/tile')"
}

echo "=========================================="
echo "raster_mask — cost by tile class and mask size"
echo "=========================================="
printf '  %-28s %s\n' "mask: borders.geojson" "16 vertices"
printf '  %-28s %s\n' "mask: germany.geojson" "84143 vertices"
echo ""

for z in $ZOOMS; do
	run "inside  · 16 vertices"    "$SMALL" "10.0,50.0,10.6,50.6"   "$z"
	run "inside  · 84k vertices"   "$BIG"   "10.0,50.0,10.6,50.6"   "$z"
	run "partial · 16 vertices"    "$SMALL" "6.0,50.0,6.6,50.6"     "$z"
	run "partial · 84k vertices"   "$BIG"   "6.0,50.0,6.6,50.6"     "$z"
	run "outside · 16 vertices"    "$SMALL" "-30.0,45.0,-29.4,45.6" "$z"
	run "outside · 84k vertices"   "$BIG"   "-30.0,45.0,-29.4,45.6" "$z"
	echo ""
done
