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

To define a pipeline, create a `.vpl` file (or pass it inline — see below) and describe the pipeline using the **VersaTiles Pipeline Language (VPL)**. Pipelines always begin with a read operation (name starts with `from_`), optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

Example:

```vpl
from_container filename="world.versatiles" | do_some_filtering | do_some_processing
```

### Inline pipelines

You don't need a `.vpl` file — a pipeline can be passed directly on the command line using the `[,vpl](…)` data-source syntax. This works anywhere a tile source is accepted (`convert`, `serve`, `probe`). Quote it in your shell, since VPL contains `|` and spaces:

```bash
# inline pipeline written to a container
versatiles convert '[,vpl](from_container filename="world.versatiles" | filter level_max=10)' out.versatiles

# the same pipeline, served live
versatiles serve '[,vpl](from_container filename="world.versatiles" | filter level_max=10)'
```

For the full data-source syntax (name/type prefixes, JSON form, credentials), run `versatiles help source`.

### Reading raw vector data

Beyond existing tile containers, pipelines can read raw vector geo data and produce MVT tiles directly. The `from_geo` operation accepts GeoJSON, line-delimited GeoJSON (`.ndjson` / `.geojsonl` / `.geojsonseq`), and Shapefile inputs; projects features to web mercator; simplifies per zoom; and emits tiles on demand:

```vpl
from_geo filename="places.geojson" layer_name="places" max_zoom=12
```

For tabular point data (CSV with explicit longitude/latitude columns) use `from_csv`. Omit `max_zoom` to let it pick automatically based on feature density:

```vpl
from_csv filename="quakes.csv" lon_column="longitude" lat_column="latitude"
```

### Reading from remote sources

`from_container` reads local files **and** remote `versatiles`/`pmtiles` containers over HTTP, HTTPS, or SFTP — only the byte ranges actually needed are fetched, so there's no need to download the whole container first:

```vpl
from_container filename="https://download.versatiles.org/osm.versatiles"
from_container filename="sftp://user@fileserver.example.org/data/world.pmtiles"
```

Credentials, ports, and authentication details are covered by `versatiles help source`.

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

## Writing or serving the result

A pipeline's output can be **written to a container** or **served live** over HTTP. Both `convert` targets and `serve` sources accept local paths as well as remote URLs:

```bash
# write to a local container
versatiles convert pipeline.vpl world.versatiles

# write the result straight to a remote SFTP server (no local copy)
versatiles convert pipeline.vpl sftp://user@fileserver.example.org/tiles/world.versatiles

# serve the pipeline output as a live tile endpoint
versatiles serve pipeline.vpl
```

Remote reading and writing support the `versatiles` and `pmtiles` formats. See `versatiles help source` for SFTP authentication and URL details.

## End-to-end examples

```bash
# read a remote pmtiles, drop high zoom levels, and write the result back to SFTP
versatiles convert \
  '[,vpl](from_container filename="https://example.org/planet.pmtiles" | filter level_max=8)' \
  sftp://user@fileserver.example.org/tiles/overview.versatiles

# merge a remote base map with a local overlay and serve it live
versatiles serve '[,vpl](from_merged_vector [
  from_container filename="https://download.versatiles.org/osm.versatiles",
  from_container filename="local-overlay.versatiles"
])'
```

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
