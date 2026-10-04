#!/usr/bin/env bash
# Smoke-test one released CLI artifact, exactly as a user would get it.
#
# Usage:
#   ./scripts/smoke-test-release.sh <tag> <asset>
#   ./scripts/smoke-test-release.sh v5.0.0-rc.2 versatiles-macos-aarch64
#
# Downloads <asset>.tar.gz from the GitHub release <tag>, checks its .sha256
# (and its build provenance, when `gh` is available), then runs the binary inside: --version, probe,
# convert to .versatiles and .pmtiles, and serve with real HTTP requests,
# including an ETag revalidation. With SMOKE_DEB=1 it also installs the .deb
# of the same name (Debian/Ubuntu, needs sudo) and runs that.
#
# Unlike selftest-versatiles.sh, which checks a binary straight out of the
# build, this checks what was published — so it also catches packaging,
# signing and upload mistakes. Needs `curl`, `tar`, and `sha256sum` or
# `shasum`; `gh` (logged in, or GH_TOKEN) adds the provenance check, which the
# release-smoke-test workflow runs once for every file on Linux instead.
# Testdata comes from this checkout.

set -euo pipefail

TAG=${1:?usage: smoke-test-release.sh <tag> <asset>}
ASSET=${2:?usage: smoke-test-release.sh <tag> <asset>}
REPO="versatiles-org/versatiles-rs"
VERSION=${TAG#v}
TESTDATA="$(cd "$(dirname "$0")/.." && pwd)/testdata"

WORK=$(mktemp -d)
SERVER_PID=""
cleanup() {
	if [ -n "$SERVER_PID" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
	rm -rf "$WORK"
}
trap cleanup EXIT
cd "$WORK"

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail() {
	printf '\033[1;31mFAILED: %s\033[0m\n' "$*" >&2
	exit 1
}

sha256_check() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum -c "$1"
	else
		shasum -a 256 -c "$1"
	fi
}

download() {
	curl --fail --silent --show-error --location --retry 3 --output "$1" \
		"https://github.com/$REPO/releases/download/$TAG/$1"
}

verify_provenance() {
	if command -v gh >/dev/null 2>&1; then
		gh attestation verify "$1" --repo "$REPO"
	else
		echo "skipped: gh is not installed"
	fi
}

step "Download $ASSET.tar.gz from $TAG"
download "$ASSET.tar.gz"
download "$ASSET.tar.gz.sha256"

step "Verify checksum"
sha256_check "$ASSET.tar.gz.sha256"

step "Verify build provenance"
verify_provenance "$ASSET.tar.gz"

step "Unpack"
tar xzf "$ASSET.tar.gz"
BIN="$WORK/versatiles"
[ -f "$BIN.exe" ] && BIN="$BIN.exe"
[ -f "$BIN" ] || fail "no versatiles binary in $ASSET.tar.gz"

# Everything below runs against the binary given here, so the .deb install
# can reuse it with the binary it put on PATH.
run_checks() {
	local bin=$1

	step "Version"
	local reported
	reported=$("$bin" --version)
	echo "$reported"
	[ "$reported" = "versatiles $VERSION" ] || fail "expected 'versatiles $VERSION', got '$reported'"

	step "Probe"
	# probe prints to stderr
	"$bin" probe "$TESTDATA/berlin.mbtiles" >probe.txt 2>&1
	grep -q 'maxzoom' probe.txt || fail "probe printed no metadata"

	step "Convert to .versatiles and .pmtiles"
	"$bin" convert --max-zoom 8 "$TESTDATA/berlin.mbtiles" out.versatiles
	"$bin" convert --max-zoom 8 out.versatiles out.pmtiles
	"$bin" probe out.pmtiles >/dev/null

	step "Serve"
	"$bin" serve -p 0 --auto-shutdown 120000 "[osm]out.versatiles" >serve.log 2>&1 &
	SERVER_PID=$!
	local port=""
	for _ in $(seq 1 60); do
		port=$(sed -n 's/^VERSATILES_PORT=\([0-9]*\).*/\1/p' serve.log | head -n 1)
		[ -n "$port" ] && break
		sleep 0.5
	done
	[ -n "$port" ] || {
		cat serve.log
		fail "server did not report a port"
	}
	local base="http://127.0.0.1:$port"

	local status
	status=$(curl -s -o tiles.json -w '%{http_code}' "$base/tiles/osm/tiles.json")
	[ "$status" = 200 ] || fail "tiles.json returned $status"
	grep -q '"maxzoom":8' tiles.json || fail "tiles.json does not say maxzoom 8"

	status=$(curl -s -D headers.txt -o tile.pbf -w '%{http_code}' "$base/tiles/osm/8/137/83")
	[ "$status" = 200 ] || fail "tile 8/137/83 returned $status"
	[ -s tile.pbf ] || fail "tile 8/137/83 is empty"

	local etag
	etag=$(sed -n 's/^[Ee][Tt][Aa][Gg]: *\(.*\)$/\1/p' headers.txt | tr -d '\r')
	[ -n "$etag" ] || fail "tile response carries no ETag"
	status=$(curl -s -o /dev/null -w '%{http_code}' -H "If-None-Match: $etag" "$base/tiles/osm/8/137/83")
	[ "$status" = 304 ] || fail "revalidating with $etag returned $status, not 304"

	kill "$SERVER_PID" 2>/dev/null || true
	wait "$SERVER_PID" 2>/dev/null || true
	SERVER_PID=""
	echo "tiles.json, tile and 304 revalidation OK on port $port"
}

run_checks "$BIN"

if [ "${SMOKE_DEB:-0}" = 1 ]; then
	DEB="${ASSET}.deb"
	step "Download and verify $DEB"
	download "$DEB"
	verify_provenance "$DEB"

	step "Install $DEB"
	SUDO=sudo
	[ "$(id -u)" = 0 ] && SUDO=""
	$SUDO dpkg -i "$DEB"
	run_checks "$(command -v versatiles)"
	$SUDO dpkg -r versatiles
fi

step "All checks passed for $ASSET ($TAG)"
