#!/usr/bin/env bash
# Pack the parts of GDAL's Homebrew tree that a build actually needs.
#
# Packing every keg `brew deps gdal` reports is ~5 GB, most of which no build
# ever touches: 1.4 GB of static archives we never link, llvm's and gcc's own
# trees (neither appears in libgdal's link closure), and 760 MB of PROJ
# datum-shift grids. What a `--features gdal` build and its tests need is much
# smaller:
#
#   1. libgdal and the dylibs it resolves to, transitively.
#   2. gdal's own keg in full — headers, `gdal-config`, driver plugins, data.
#   3. PROJ's data except the `.tif` grids: `proj.db` and the CRS definitions
#      are needed for any coordinate work; the grids only refine datum shifts,
#      and PROJ falls back to a ballpark transform without them.
#   4. Every keg's `INSTALL_RECEIPT.json`. These are what make `brew install
#      gdal` still see each dependency as installed, so it relinks instead of
#      re-pouring ~70 bottles — which is the whole point of the cache.
#   5. The `opt/<formula>` symlinks, which is how libgdal finds its
#      dependencies at runtime.
#
# Result: ~190 MB instead of ~5 GB.

set -euo pipefail

PREFIX="$(brew --prefix)"
OUT="${1:-$HOME/gdal-brew-tree.tar}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
seen="$work/seen"
queue="$work/queue"
: >"$seen"

# Transitive dylib closure of libgdal, restricted to the Homebrew prefix.
# Plain files rather than an associative array: macOS ships bash 3.2.
echo "$PREFIX/lib/libgdal.dylib" >"$queue"
while [ -s "$queue" ]; do
	lib="$(head -n 1 "$queue")"
	sed -i '' '1d' "$queue"
	real="$(readlink -f "$lib" 2>/dev/null || true)"
	[ -n "$real" ] && [ -f "$real" ] || continue
	grep -qxF "$real" "$seen" && continue
	echo "$real" >>"$seen"
	otool -L "$real" 2>/dev/null | tail -n +2 | awk '{print $1}' | grep "^$PREFIX/" >>"$queue" || true
done

# tar members are prefix-relative, so it can unpack with `-C /`.
members="$work/members"
sed "s|^$PREFIX/|${PREFIX#/}/|" "$seen" >"$members"

for formula in gdal $(brew deps gdal); do
	keg="$PREFIX/Cellar/$formula"
	[ -d "$keg" ] || continue

	# Only files and symlinks are listed, never directories: tar recurses into
	# any directory it is given, which would drag the whole keg back in and
	# archive every listed file a second time.
	if [ "$formula" = "gdal" ]; then
		find "$keg" \( -type f -o -type l \) -print
	elif [ "$formula" = "proj" ]; then
		# Everything but the datum-shift grids.
		find "$keg" -type f -name INSTALL_RECEIPT.json -print
		find "$keg/"*/share/proj \( -type f -o -type l \) ! -name '*.tif' -print 2>/dev/null || true
	else
		find "$keg" -type f -name INSTALL_RECEIPT.json -print
	fi

	# Symlinks beside each closure member (libfoo.dylib -> libfoo.3.dylib), and
	# the opt/ entry the dependency is reached through.
	find "$keg" -type l -name '*.dylib' -print 2>/dev/null || true
	[ -e "$PREFIX/opt/$formula" ] && echo "$PREFIX/opt/$formula"
done | sed "s|^$PREFIX/|${PREFIX#/}/|" >>"$members"

sort -u "$members" -o "$members"

# A cache that is missing libraries fails on the *next* run, in another job,
# with a link error that says nothing about packing. Fail here instead.
libs="$(wc -l <"$seen" | tr -d ' ')"
if [ "$libs" -lt 50 ]; then
	echo "only $libs libraries in libgdal's link closure — expected ~180; refusing to pack" >&2
	exit 1
fi
if ! grep -q '/bin/gdal-config$' "$members"; then
	echo "gdal-config is not in the member list; refusing to pack" >&2
	exit 1
fi

echo "packing $(wc -l <"$members" | tr -d ' ') paths ($libs libraries in the link closure)"

# -h is deliberately absent: symlinks are stored as symlinks, not followed.
tar -cf "$OUT" -C / -T "$members"
echo "wrote $OUT ($(du -h "$OUT" | cut -f1))"
