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

A pipeline does not have to start from an existing tile container: `from_geo` reads GeoJSON, line-delimited GeoJSON and Shapefiles, and `from_csv` reads tabular point data, both emitting MVT tiles on demand.

```vpl
from_geo filename="places.geojson" layer_name="places" max_zoom=12
```

### Generating grid cells

Gridded statistics — population per cell, sensor readings, anything aggregated to a regular tessellation — are published as tables keyed on a cell id, without the geometry that id refers to. `from_grid` and `from_h3` generate that geometry, so the table can be joined onto it with `vector_update_properties`:

```vpl
from_grid epsg=3035 size=1000 bbox=[5.8,47.2,15.1,55.1]
  | vector_update_properties data_source_path="population.csv"
      id_field_tiles="id" id_field_data="GRD_ID"
```

`from_grid` produces squares of a fixed size in a projected CRS, `from_h3` produces H3 hexagons addressed by resolution. See their own sections for ids, projections and the zoom level each starts at.

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
