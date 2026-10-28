#!/usr/bin/env bash
# Check that every crate the workspace publishes contains a LICENSE (#292).
#
# The MIT licence asks for its notice in every copy, and a crate on crates.io is
# one. Each crate has `LICENSE` as a symlink to the root file, which
# `cargo package` stores as a regular file. A new crate without one fails here.
#
# The published crates are the workspace members without `publish = false`.

cd "$(dirname "$0")/.."

set -euo pipefail

crates=$(cargo metadata --no-deps --format-version 1 2>/dev/null |
	python3 -I -c 'import json, sys; print(*(p["name"] for p in json.load(sys.stdin)["packages"] if p["publish"] != []))')

missing=0
for crate in $crates; do
	# Not `grep -q`: it exits at the first match, and with `pipefail` the
	# SIGPIPE that leaves `cargo` with would count as a missing file.
	if ! cargo package --list --allow-dirty -p "$crate" 2>/dev/null | grep -x LICENSE >/dev/null; then
		echo "$crate: no LICENSE in the package"
		missing=1
	fi
done

if [ "$missing" -eq 0 ]; then
	echo "LICENSE is in every published crate: $crates"
fi
exit "$missing"
