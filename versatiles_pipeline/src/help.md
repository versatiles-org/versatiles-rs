# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles. It uses multithreading to stream, process, and transform tiles from one or more sources in parallel, either for storing them in a new tile container or delivering them in real-time through a server:

```bash
# save the processed tiles in a container:
versatiles convert pipeline.vpl result.versatiles

# serve the tiles directy via the server:
versatiles serve pipeline.vpl
```

## Defining a pipeline

To define a pipeline, create a `.vpl` file and describe the pipeline using the **VersaTiles Pipeline Language (VPL)**. Pipelines always begin with a read operation (name starts with `from_`), optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

Example:

```vpl
from_container filename="world.versatiles" | do_some_filtering | do_some_processing
```

### Reading raw vector data

Beyond existing tile containers, pipelines can read raw vector geo data and produce MVT tiles directly. The `from_geo` operation accepts GeoJSON and Shapefile inputs, projects features to web mercator, simplifies per zoom, and emits tiles on demand:

```vpl
from_geo filename="places.geojson" layer_name="places" max_zoom=12
```

For tabular point data (CSV with explicit longitude/latitude columns) use `from_csv`. Omit `max_zoom` to let it pick automatically based on feature density:

```vpl
from_csv filename="quakes.csv" lon_column="longitude" lat_column="latitude"
```

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
