# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles. It uses multithreading to stream, process, and transform tiles from one or more sources in parallel, either for storing them in a new tile container or delivering them in real-time through a server:

```bash
# save the processed tiles in a container:
versatiles convert pipeline.vpl result.versatiles

# serve the tiles directy via the server:
versatiles serve pipeline.vpl
```

<!-- VPL_OPERATIONS_TOC -->

## Defining a pipeline

To define a pipeline, create a `.vpl` file and describe the pipeline using the **VersaTiles Pipeline Language (VPL)**. Pipelines always begin with a read operation (name starts with `from_`), optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

Example:

```vpl
from_container filename="world.versatiles" | do_some_filtering | do_some_processing
```

A pipeline can also be passed inline on the command line — and written to a local or remote target, or served live — without a `.vpl` file. See [Usage in the main README](https://github.com/versatiles-org/versatiles-rs#usage) and `versatiles help source` for CLI invocation and data-source syntax.

### Reading raw vector data

Beyond existing tile containers, pipelines can read raw vector geo data and produce MVT tiles directly. The `from_geo` operation accepts GeoJSON, line-delimited GeoJSON (`.ndjson` / `.geojsonl` / `.geojsonseq`), and Shapefile inputs; projects features to web mercator; simplifies per zoom; and emits tiles on demand:

```vpl
from_geo filename="places.geojson" layer_name="places" max_zoom=12
```

For tabular point data (CSV with explicit longitude/latitude columns) use `from_csv`. Omit `max_zoom` to let it pick automatically based on feature density:

```vpl
from_csv filename="quakes.csv" lon_column="longitude" lat_column="latitude"
```

### Generating grid cells

Gridded statistics — population per cell, sensor readings, anything aggregated to a regular tessellation — are published as tables keyed on a cell id, without the geometry that id refers to. `from_grid` and `from_h3` generate that geometry, so the table can be joined onto it with `vector_update_properties`:

```vpl
from_grid epsg=3035 size=1000 bbox=[5.8,47.2,15.1,55.1]
  | vector_update_properties data_source_path="population.csv"
      id_field_tiles="id" id_field_data="GRD_ID"
```

`from_grid` produces squares of a fixed size in a projected CRS. The id follows the INSPIRE form Eurostat and its member states publish (`CRS3035RES1000mN2691000E4341000`); `id_preset="geostat"` gives the short form (`1kmN2689E4337`), and `id_template=` spells out anything else — `E{x/100:04}N{y/100:04}` produces `E0643N4567`, the form Dutch grid statistics use. Each cell also carries its lower-left corner as `x` and `y`, mirroring the `X_LLC` / `Y_LLC` columns published beside the ids.

Without GDAL, `epsg` accepts 3035 (ETRS89-LAEA, what European gridded statistics use), 3857 and 4326; a build with the `gdal` feature accepts any code, at roughly ten times the cost per coordinate. Released binaries ship without GDAL, so a `.vpl` naming another code will not run on one.

`from_h3` produces H3 hexagons instead, addressed by resolution rather than size, and carries the H3 index as `h3` — the column name H3 datasets usually use:

```vpl
from_h3 resolution=8 bbox=[13.0,52.3,13.8,52.7]
  | vector_update_properties data_source_path="kontur_population.csv"
      id_field_tiles="h3" id_field_data="h3"
```

Both require a `bbox` and both derive their own minimum zoom: cell size is fixed, so at low zoom one tile would hold every cell in view. `max_cells_per_tile` moves that threshold. A cell reaching into several tiles is drawn in each of them, clipped to the tile — so the same id recurs across tiles, which a join expects, but the geometry in one tile is only the part that falls inside it.

### Reading from remote sources

`from_container` also reads remote `versatiles`/`pmtiles` containers over HTTP, HTTPS, or SFTP (e.g. `filename="https://download.versatiles.org/osm.versatiles"`), fetching only the byte ranges it needs. See `versatiles help source` for URL and authentication details.

## Operation Format

Each operation follows this structure:

```vpl
operation_name parameter1="value1" parameter2="value2" ...
```

For read operations that combine multiple sources, use a comma-separated list within square brackets:

Example:

```vpl
from_stacked [
   from_container filename="world.versatiles",
   from_container filename="europe.versatiles" | filter level_min=5,
   from_container filename="germany.versatiles"
]
```

### Parameter values

A value can be written in three ways:

- **bare** — letters, digits and `.`, `-`, `_`, e.g. `level_min=5`, `format=webp`, `nodata=-0.5`. Anything else (spaces, `=`, `|`, `[`, `]`, `,`, quotes) has to be quoted.
- **single-quoted** — `'…'` is taken literally up to the next `'`. There are no escapes, so a single-quoted value cannot itself contain `'`.
- **double-quoted** — `"…"` supports the escapes `\\`, `\"`, `\n` and `\t`. Use this form for a value containing a single quote, e.g. `expr="name == 'Berlin'"`.

Both quoted forms may be empty: `""` and `''` are the empty string. That is distinct from omitting the parameter — an absent parameter falls back to its default, whereas an empty one is a value that the operation receives and validates like any other, so operations expecting a number or a filename will reject it.

To pass several values to one parameter, use a comma-separated list in square brackets, e.g. `layer=["place", "water"]`. The same three forms apply to each element.

Every parameter that takes a file path resolves a relative one against the `.vpl` file's own directory, so a pipeline and the data it reads can be moved together. The individual parameters below do not repeat this.

## Filter expressions (CEL)

The `vector_filter_features` transform evaluates a boolean [CEL (Common Expression Language)](https://github.com/google/cel-spec) expression per feature. Quick reference:

### Types

- **bool** — `true`, `false`
- **int / uint** — `42`, `-7`, `1000u`
- **double** — `3.14`, `-0.5`, `1e-6`
- **string** — `'hello'` or `"hello"`
- **list** — `[1, 2, 3]`, `['a', 'b']`
- **map** — accessed via `m['key']` or `m.key`
- **null** — `null`

### Operators

- **Equality** — `==`, `!=`
- **Ordering** — `<`, `<=`, `>`, `>=`
- **Logical** — `&&`, `||`, `!`
- **Membership** — `x in [1, 2, 3]`
- **Regex** — `s.matches('pattern')` (RE2 syntax, matched anywhere in `s`)

### Accessing feature properties

Properties whose names are valid CEL identifiers (letters, digits, underscore) are exposed as top-level variables:

```vpl
vector_filter_features layer=["place"] expr="name == 'Berlin'"
```

For keys containing `:`, `-`, `.`, or other non-identifier characters, use the `props` map:

```vpl
vector_filter_features layer=["addr"] expr="props['addr:street'] == 'Hauptstr.'"
```

### Missing keys

A property absent from a feature resolves to `null` for identifier-safe access. Compare against `null` to keep or drop missing-key features explicitly:

```vpl
# keep only features whose `name` is present and non-empty
vector_filter_features layer=["place"] expr="name != null && name != ''"
```

For identifier-safe keys you can also use the `has()` macro on the `props` map:

```vpl
# equivalent presence check on an identifier-safe key
vector_filter_features layer=["place"] expr="has(props.name)"
```

For non-identifier keys (containing `:`, `-`, `.`, etc.), use the `in` operator:

```vpl
vector_filter_features layer=["addr"] expr="'addr:street' in props"
```

### More

See the [CEL language spec](https://github.com/google/cel-spec/blob/master/doc/langdef.md) for the full grammar, built-in functions, and string methods.
