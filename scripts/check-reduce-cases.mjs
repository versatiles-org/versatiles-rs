// Checks the `drawn` of every vector_reduce_to_style case against MapLibre.
//
// Usage: node scripts/check-reduce-cases.mjs
//
// The cases in versatiles_pipeline/tests/reduce_cases/ state by hand which
// features MapLibre draws. This asks MapLibre's own style-spec package instead:
// it resolves `ref` layers with `derefLayers`, evaluates each layer's filter
// with `featureFilter` at every map zoom that loads the case's tile zoom, and
// applies the rules around it — layer zoom bounds, visibility, which geometries
// a layer type draws, the source's zoom range. A case whose `drawn` features
// differ is reported. Properties are not checked: the package does not say
// which properties a layer reads.
//
// Skipped, and counted: cases expecting an error, cases whose widening is
// `draws-nothing` (whether a feature is visible depends on paint and icons,
// which this does not evaluate), cases with `global-state`, whose answer
// changes at runtime, and cases whose style MapLibre refuses — those are
// listed, since a refused style may be a broken case.
//
// The package is installed into target/ on first use, at a pinned version.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STYLE_SPEC = "@maplibre/maplibre-gl-style-spec@26.4.4";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const casesDir = path.join(root, "versatiles_pipeline/tests/reduce_cases");
const prefix = path.join(root, "target/reduce-cases-oracle");

if (
  !existsSync(
    path.join(prefix, "node_modules/@maplibre/maplibre-gl-style-spec"),
  )
) {
  mkdirSync(prefix, { recursive: true });
  execFileSync(
    "npm",
    ["install", "--prefix", prefix, "--no-save", "--silent", STYLE_SPEC],
    { stdio: "inherit" },
  );
}
const { featureFilter, derefLayers } = createRequire(
  path.join(prefix, "index.js"),
)("@maplibre/maplibre-gl-style-spec");

/** MVT geometry type numbers, as `featureFilter` expects them. */
const GEOMETRY = { Point: 1, LineString: 2, Polygon: 3 };

/** The geometries a layer type draws; a type not listed draws all of them. */
const DRAWS = { fill: [3], "fill-extrusion": [3], line: [2, 3] };

/** Map zooms are sampled this finely between whole zooms. */
const STEPS = [0, 0.25, 0.5, 0.75, 0.999];

/** How far up a map can zoom; overzoomed tiles are drawn up to here. */
const MAP_MAXZOOM = 24;

let checked = 0;
const skipped = {
  error: 0,
  "draws-nothing": 0,
  "global-state": 0,
  "refused by MapLibre": 0,
};
const refused = [];
const failures = [];

// The package warns on the console about filters it evaluates to false, such as a comparison
// with a missing property. That is the evaluation working, not a problem with a case.
console.warn = () => {};

for (const file of readdirSync(casesDir)
  .filter((f) => f.endsWith(".json"))
  .sort()) {
  for (const testCase of JSON.parse(
    readFileSync(path.join(casesDir, file), "utf8"),
  )) {
    const where = `${file}: ${JSON.stringify(testCase.name)}`;
    if (testCase.error !== undefined) {
      skipped.error++;
      continue;
    }
    if (testCase.widening?.kind === "draws-nothing") {
      skipped["draws-nothing"]++;
      continue;
    }
    if (JSON.stringify(testCase.style).includes('"global-state"')) {
      skipped["global-state"]++;
      continue;
    }
    const zooms = Array.isArray(testCase.zoom)
      ? testCase.zoom
      : [testCase.zoom];
    for (const zoom of zooms) {
      const expected = drawnFeatures(
        Array.isArray(testCase.zoom)
          ? testCase.drawn[String(zoom)]
          : testCase.drawn,
      );
      let actual;
      try {
        actual = maplibreDraws(testCase, zoom);
      } catch (error) {
        skipped["refused by MapLibre"]++;
        refused.push(`${where}: ${error.message}`);
        break;
      }
      if (zoom === zooms[0]) checked++;
      const at = zooms.length > 1 ? ` at z${zoom}` : "";
      const missing = difference(actual, expected);
      const extra = difference(expected, actual);
      if (missing.length > 0)
        failures.push(
          `${where}: MapLibre draws${at} what \`drawn\` lacks: ${missing.join(", ")}`,
        );
      if (extra.length > 0)
        failures.push(
          `${where}: \`drawn\` has${at} what MapLibre does not draw: ${extra.join(", ")}`,
        );
    }
  }
}

console.log(
  `${checked} cases checked against ${STYLE_SPEC}; skipped: ${Object.entries(
    skipped,
  )
    .map(([why, n]) => `${n} ${why}`)
    .join(", ")}`,
);
if (refused.length > 0)
  console.log(`\nRefused by MapLibre:\n${refused.join("\n")}`);
if (failures.length > 0) {
  console.error(
    `\n${failures.length} disagreements:\n\n${failures.join("\n")}`,
  );
  process.exit(1);
}

/** The features a `drawn` lists, as "layer/id". */
function drawnFeatures(content) {
  return Object.entries(content).flatMap(([layer, features]) =>
    Object.keys(features).map((id) => `${layer}/${id}`),
  );
}

/** What `a` has that `b` lacks. */
function difference(a, b) {
  const set = new Set(b);
  return a.filter((x) => !set.has(x));
}

/** The features MapLibre draws from the case's tile at tile zoom `zoom`, as "layer/id". */
function maplibreDraws(testCase, zoom) {
  const style = Array.isArray(testCase.style)
    ? { layers: testCase.style }
    : testCase.style;
  const sources = style.sources ?? {};
  const tilesetMaxzoom = testCase.tileset_maxzoom ?? 14;
  const selected = selectedSources(testCase.source, sources);

  const layers = derefLayers(
    style.layers.map((layer, i) => ({ id: `layer-${i}`, ...layer })),
  );
  const drawn = new Set();

  for (const [index, layer] of layers.entries()) {
    if (layer["source-layer"] === undefined) continue;
    if (selected !== null && !selected.has(layer.source)) continue;
    if (layer.layout?.visibility === "none") continue;

    // The tile zooms MapLibre requests from this layer's source.
    const definition = sources[layer.source] ?? {};
    const sourceMin = definition.minzoom ?? 0;
    const sourceMax = Math.min(
      definition.maxzoom ?? tilesetMaxzoom,
      tilesetMaxzoom,
    );
    if (zoom < sourceMin || zoom > sourceMax) continue;

    // The map zooms at which this tile is the one drawn: its own zoom, and above it when it is
    // the deepest the source has.
    const top = zoom === sourceMax ? MAP_MAXZOOM : zoom;
    const mapZooms = [];
    for (let z = zoom; z <= top; z++)
      for (const step of STEPS) mapZooms.push(z + step);
    const visibleAt = mapZooms.filter(
      (z) => z >= (layer.minzoom ?? 0) && z < (layer.maxzoom ?? Infinity),
    );
    if (visibleAt.length === 0) continue;

    const filter = featureFilter(layer.filter, `layers[${index}].filter`);
    const geometries = DRAWS[layer.type] ?? [1, 2, 3];

    for (const feature of testCase.tile[layer["source-layer"]] ?? []) {
      const type = GEOMETRY[feature.$geometry ?? "Point"];
      if (!geometries.includes(type)) continue;
      const properties = Object.fromEntries(
        Object.entries(feature)
          .filter(([key]) => key !== "id" && key !== "$geometry")
          .map(([key, value]) => [key, plain(value)]),
      );
      const evaluated = { type, id: feature.id, properties };
      if (visibleAt.some((z) => filter.filter({ zoom: z }, evaluated))) {
        drawn.add(`${layer["source-layer"]}/${feature.id}`);
      }
    }
  }
  return [...drawn];
}

/** A typed number such as `{"uint": 2}` is just a number to MapLibre. */
function plain(value) {
  if (value !== null && typeof value === "object")
    return Object.values(value)[0];
  return value;
}

/**
 * The source IDs whose layers count, or `null` for all: the named ones, and every source with the
 * same `url` or `tiles` as one of them.
 */
function selectedSources(named, sources) {
  if (!named || named.length === 0) return null;
  const identity = (id) => {
    const d = sources[id] ?? {};
    return d.url
      ? `url ${d.url}`
      : d.tiles
        ? `tiles ${JSON.stringify(d.tiles)}`
        : `source ${id}`;
  };
  const wanted = new Set(named.map(identity));
  return new Set(
    [...Object.keys(sources), ...named].filter((id) =>
      wanted.has(identity(id)),
    ),
  );
}
