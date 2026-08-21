#!/usr/bin/env python3
"""Check the embedded Shortbread YAML against the published specification.

The `shortbread_*.yaml` files next to this script are hand-maintained: `required`
and `enum_severity` are validator policy, not facts the spec states, so nothing
can generate them. What *can* be checked automatically is everything the spec
does state — the layer set, each layer's geometry and minimum zoom, its property
names and types, and the `kind` vocabulary — and that is what drifts.

    python3 check_schema.py            # check every embedded version
    python3 check_schema.py 1.1        # check one
    python3 check_schema.py --cache d  # reuse/save the fetched HTML in dir `d`

Exits non-zero when the YAML and the spec disagree. Each layer section of the
spec carries two tables this reads:

  Properties: Field Name | Type | ...        -> attribute names and types
  Features:   Feature | Value of `kind` | OSM tag | Geometry | Zoom
                                              -> kind vocabulary, geometry, minzoom

Layers whose `kind` the spec defines by reference ("see the water_polygons layer
for a list of values") have no Features table of their own; `INHERITED_KIND` maps
them to the layer they borrow from.
"""

import argparse
import html
import os
import re
import sys
import urllib.request

BASE_URL = "https://shortbread-tiles.org/schema/{version}/"
VERSIONS = ["1.0", "1.1"]

# Layers whose `kind` values the spec defines by pointing at another layer.
INHERITED_KIND = {
    "water_polygons_labels": "water_polygons",
    "water_lines_labels": "water_lines",
}

# Attributes the spec states in prose rather than in a Properties table, so this
# script cannot see them. `buildings` has no table at all: "There is a property
# for all features on this layer, called dummy, a number which is always 1."
PROSE_ATTRIBUTES = {("buildings", "dummy")}

TYPE_MAP = {"string": "String", "integer": "Number", "float": "Number", "boolean": "Boolean"}
GEOMETRY_MAP = {"point": "Point", "line": "Line", "polygon": "Polygon"}


# --- fetching -------------------------------------------------------------


def fetch(version, cache_dir):
    path = os.path.join(cache_dir, f"shortbread_{version}.html") if cache_dir else None
    if path and os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            return f.read()
    url = BASE_URL.format(version=version)
    with urllib.request.urlopen(url, timeout=60) as response:
        text = response.read().decode("utf-8")
    if path:
        os.makedirs(cache_dir, exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
    return text


# --- spec parsing ---------------------------------------------------------


def strip_tags(fragment):
    return html.unescape(re.sub(r"<[^>]+>", " ", fragment)).strip()


def parse_tables(section):
    """Every table in `section`, as (lowercased headers, list of row cells)."""
    for table in re.findall(r"<table>(.*?)</table>", section, re.S):
        headers = [strip_tags(h).lower() for h in re.findall(r"<th[^>]*>(.*?)</th>", table, re.S)]
        rows = []
        for row in re.findall(r"<tr>(.*?)</tr>", table, re.S):
            cells = [strip_tags(c) for c in re.findall(r"<td[^>]*>(.*?)</td>", row, re.S)]
            if cells:
                rows.append(cells)
        yield headers, rows


def parse_zoom(cell):
    """The lowest zoom a Features row is available at, or None if unstated.

    Most cells read `10+`. Some are prose — "available if their line is longer
    than 0.25 pixel but not below 12" — where the only zoom stated is the floor,
    and the other numbers in the sentence are not zooms at all.
    """
    match = re.match(r"^(\d+)\s*\+", cell)
    if match:
        return int(match.group(1))
    match = re.search(r"not below\s+(\d+)", cell)
    if match:
        return int(match.group(1))
    return int(cell) if cell.strip().isdigit() else None


def parse_spec(page):
    """{layer: {attributes, kinds, geometry, minzoom}} from one spec page."""
    layers = {}
    parts = re.split(r'id="layer-([a-z_]+)"', page)
    for i in range(1, len(parts), 2):
        name, section = parts[i], parts[i + 1]
        layer = {"attributes": {}, "kinds": None, "geometry": None, "minzoom": None}
        for headers, rows in parse_tables(section):
            if not headers:
                continue
            if headers[0].startswith("field"):
                for cells in rows:
                    if len(cells) >= 2:
                        layer["attributes"][cells[0]] = cells[1].lower()
            # The first column is "Feature" on some layers and "Type of feature"
            # on others, so key off the `kind` column instead.
            elif len(headers) >= 2 and headers[1].startswith("value of") and "kind" in headers[1]:
                kinds, geometries, zooms = [], [], []
                for cells in rows:
                    if len(cells) >= 2 and cells[1]:
                        kinds.extend(v.strip() for v in cells[1].split(",") if v.strip())
                    if len(cells) >= 4:
                        geometries.append(cells[3].lower())
                    if len(cells) >= 5:
                        zoom = parse_zoom(cells[4])
                        if zoom is not None:
                            zooms.append(zoom)
                # Dedupe while keeping spec order.
                layer["kinds"] = list(dict.fromkeys(kinds))
                for geometry in geometries:
                    if geometry in GEOMETRY_MAP:
                        layer["geometry"] = GEOMETRY_MAP[geometry]
                        break
                if zooms:
                    layer["minzoom"] = min(zooms)
        layers[name] = layer

    for borrower, lender in INHERITED_KIND.items():
        if borrower in layers and layers[borrower]["kinds"] is None and lender in layers:
            layers[borrower]["kinds"] = layers[lender]["kinds"]
    return layers


# --- YAML parsing ---------------------------------------------------------
#
# A dependency-free reader for the fixed shape these two files have, so the
# check runs anywhere python3 does. It is not a general YAML parser.


def parse_yaml(path):
    layers = {}
    layer = attribute = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            indent = len(line) - len(line.lstrip())
            stripped = line.strip()
            # A quoted key may itself contain a colon ("recycling:paper"), so the
            # separator is the first colon *outside* the quotes.
            if stripped.startswith('"'):
                key, _, value = stripped[1:].partition('":')
            else:
                key, _, value = stripped.partition(":")
            key, value = key.strip().strip('"'), value.strip()
            if indent == 2:
                layer, attribute = key, None
                layers[layer] = {"attributes": {}}
            elif indent == 4 and layer:
                if key == "attributes":
                    attribute = None
                else:
                    layers[layer][key] = value.strip('"')
            elif indent == 6 and layer:
                attribute = key
                layers[layer]["attributes"][attribute] = {}
            elif indent == 8 and attribute:
                if key == "enum":
                    value = [v.strip().strip('"') for v in value.strip("[]").split(",")]
                layers[layer]["attributes"][attribute][key] = value
    return layers


# --- comparison -----------------------------------------------------------


def check(version, cache_dir):
    here = os.path.dirname(os.path.abspath(__file__))
    yaml_path = os.path.join(here, f"shortbread_{version.replace('.', '_')}.yaml")
    embedded = parse_yaml(yaml_path)
    spec = parse_spec(fetch(version, cache_dir))
    problems = []

    def report(message):
        problems.append(f"{version}: {message}")

    for name in sorted(set(spec) | set(embedded)):
        if name not in embedded:
            report(f"{name}: in the spec, missing from the YAML")
            continue
        if name not in spec:
            report(f"{name}: in the YAML, not in the spec")
            continue
        want, have = spec[name], embedded[name]

        if want["geometry"] and want["geometry"] != have.get("geometry"):
            report(f"{name}: geometry is {have.get('geometry')}, spec says {want['geometry']}")
        if want["minzoom"] is not None and str(want["minzoom"]) != have.get("minzoom"):
            report(f"{name}: minzoom is {have.get('minzoom')}, spec says {want['minzoom']}")

        for attribute, spec_type in want["attributes"].items():
            if attribute not in have["attributes"]:
                report(f"{name}.{attribute}: in the spec ({spec_type}), missing from the YAML")
                continue
            expected = TYPE_MAP.get(spec_type)
            actual = have["attributes"][attribute].get("type")
            if expected and expected != actual:
                report(f"{name}.{attribute}: type is {actual}, spec says {spec_type} ({expected})")
        for attribute in have["attributes"]:
            if attribute not in want["attributes"] and (name, attribute) not in PROSE_ATTRIBUTES:
                report(f"{name}.{attribute}: in the YAML, not in the spec")

        if want["kinds"]:
            declared = have["attributes"].get("kind", {}).get("enum")
            if declared is None:
                report(f"{name}.kind: no enum in the YAML; spec lists {len(want['kinds'])} values")
            else:
                for value in want["kinds"]:
                    if value not in declared:
                        report(f"{name}.kind: spec value {value!r} is not in the YAML enum")
                for value in declared:
                    if value not in want["kinds"]:
                        report(f"{name}.kind: YAML enum value {value!r} is not in the spec")

    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("versions", nargs="*", default=VERSIONS, help=f"default: {' '.join(VERSIONS)}")
    parser.add_argument("--cache", metavar="DIR", help="reuse and store the fetched spec pages here")
    args = parser.parse_args()

    problems = []
    for version in args.versions:
        problems.extend(check(version, args.cache))

    if not problems:
        print(f"shortbread {', '.join(args.versions)}: the embedded schema matches the spec")
        return 0
    for problem in problems:
        print(problem)
    print(f"\n{len(problems)} difference(s)")
    return 1


if __name__ == "__main__":
    sys.exit(main())
