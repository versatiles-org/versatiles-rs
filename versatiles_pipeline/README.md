# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles. It uses multithreading to stream, process, and transform tiles from one or more sources in parallel, either for storing them in a new tile container or delivering them in real-time through a server:

```bash
# save the processed tiles in a container:
versatiles convert pipeline.vpl result.versatiles

# serve the tiles directy via the server:
versatiles serve pipeline.vpl
```

## Operations

**Read:** [`from_color`](#from_color) · [`from_container`](#from_container) · [`from_csv`](#from_csv) · [`from_debug`](#from_debug) · [`from_gdal_dem`](#from_gdal_dem) · [`from_gdal_raster`](#from_gdal_raster) · [`from_geo`](#from_geo) · [`from_grid`](#from_grid) · [`from_h3`](#from_h3) · [`from_merged_vector`](#from_merged_vector) · [`from_stacked`](#from_stacked) · [`from_stacked_raster`](#from_stacked_raster) · [`from_tile`](#from_tile) · [`from_tilejson`](#from_tilejson)

**Transform:** [`dem_overview`](#dem_overview) · [`dem_quantize`](#dem_quantize) · [`dem_tile_resize`](#dem_tile_resize) · [`filter`](#filter) · [`meta_update`](#meta_update) · [`raster_flatten`](#raster_flatten) · [`raster_format`](#raster_format) · [`raster_levels`](#raster_levels) · [`raster_mask`](#raster_mask) · [`raster_overscale`](#raster_overscale) · [`raster_overview`](#raster_overview) · [`raster_tile_resize`](#raster_tile_resize) · [`remap_coords`](#remap_coords) · [`vector_filter_features`](#vector_filter_features) · [`vector_filter_layers`](#vector_filter_layers) · [`vector_filter_properties`](#vector_filter_properties) · [`vector_overzoom`](#vector_overzoom) · [`vector_repair`](#vector_repair) · [`vector_update_properties`](#vector_update_properties)

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

---

# READ operations

---

## from_color

Generates solid-color tiles of the specified size and format.

### Parameters

- *`color`: String (optional)* - Hex color in RGB or RGBA format (e.g., "FF5733" or "FF573380"). Defaults to "000000" (black).
- *`size`: u16 (optional)* - Tile size in pixels (256 or 512). Defaults to 512.
- *`format`: TileFormat (optional)* - Tile format: one of "avif", "jpg", "png", or "webp". Defaults to "png".

---

## from_container

Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.

### Parameters

- **`filename`: String (required)** - The filename of the tile container (relative to the VPL file path), or a URL (`http`, `https`, or `sftp`). For example: `filename="world.versatiles"` or `filename="https://example.com/world.versatiles"`. See `versatiles help source` for URL and authentication details.
- *`ssh_identity`: String (optional)* - The private key file to authenticate this `sftp://` source with, for example `ssh_identity="/home/deploy/.ssh/id_ed25519"`. A relative path resolves against the VPL file, like `filename`. It applies to this source alone and overrides `--ssh-identity` and `VERSATILES_SSH_IDENTITY`, which apply to every source — so one pipeline can read from two SFTP hosts that need different keys. Ignored for other schemes. Note that naming a key makes the VPL file specific to machines that have it.

---

## from_csv

Reads a CSV file with longitude/latitude columns and emits MVT point tiles.

### Parameters

- **`filename`: String (required)** - Filename of the CSV file (relative to the VPL file path).
- **`lon_column`: String (required)** - Header column name holding the longitude (degrees, WGS84). Required.
- **`lat_column`: String (required)** - Header column name holding the latitude (degrees, WGS84). Required.
- *`id_column`: String (optional)* - Optional column to expose as the MVT feature `id` (numeric if it parses as `u64`, else string).
- *`delimiter`: String (optional)* - Field delimiter as a single ASCII character. Defaults to `,`.
- *`has_header`: bool (optional)* - Whether row 1 contains column names. Defaults to `true`. Header-less CSVs aren't supported in v1.
- *`layer_name`: String (optional)* - Name of the MVT layer in the output tiles. Defaults to the filename stem.
- *`min_zoom`: u8 (optional)* - Lowest zoom level emitted (default 0).
- *`max_zoom`: u8 (optional)* - Highest zoom level emitted. Defaults to an auto-heuristic (median feature size ≈ 4 tile-pixels, capped at 14). For point-only inputs the heuristic returns 14.
- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding-box clip in degrees `[w, s, e, n]`. Not supported in v1; setting this errors out.
- *`properties_include`: [String,...] (optional)* - Property whitelist: keep only the named columns as feature properties, drop everything else. Mutually exclusive with `properties_exclude`. (`lon_column` / `lat_column` / `id_column` are consumed earlier by the CSV adapter and aren't affected.)
- *`properties_exclude`: [String,...] (optional)* - Property blacklist: drop the named properties, keep everything else. Mutually exclusive with `properties_include`.
- *`point_reduction`: PointReductionStrategy (optional)* - Point reduction strategy: `none` / `drop_rate` / `min_distance` (default `min_distance`).
- *`point_reduction_value`: f32 (optional)* - Numeric value whose meaning depends on `point_reduction`: - `min_distance` (default): minimum distance between kept points, in tile-pixels at the current zoom. Defaults to 16. - `drop_rate`: per-zoom keep-fraction in `[0, 1]`. Defaults to 0.5. - `none`: ignored.
- *`compression`: TileCompression (optional)* - Tile-compression applied before the tiles leave this operation: `gzip` (default), `brotli`, `zstd`, or `none`.
- *`max_tile_bytes`: u32 | none (optional)* - Maximum encoded tile size in bytes before a tile is considered broken and dropped (streaming path) / errors out (single-tile path). Defaults to 1048576 (1 MiB). Raise it when a legitimate low-zoom tile exceeds the default (e.g. `max_tile_bytes=2097152` for 2 MiB), or set `max_tile_bytes=none` to emit tiles at any size. The soft-cap warning threshold (200 KB at the default cap) scales with this value.

---

## from_debug

Generates debug tiles that display their coordinates as text.

### Parameters

- *`format`: TileFormat (optional)* - Target tile format: one of `"mvt"` (default), `"avif"`, `"jpg"`, `"png"` or `"webp"`

---

## from_gdal_dem

Reads a GDAL DEM dataset and produces terrain RGB tiles (Mapbox or Terrarium encoding).

### Parameters

- **`filename`: String (required)** - The filename of the GDAL DEM dataset to read. For example: `filename="dem.tif"`.
- *`encoding`: String (optional)* - The DEM encoding format: `"mapbox"` or `"terrarium"`. (default: `"mapbox"`)
- *`tile_size`: u32 (optional)* - The size of the generated tiles in pixels. (default: 512)
- *`level_max`: u8 (optional)* - The maximum zoom level to generate tiles for. (default: the maximum zoom level based on the dataset's native resolution)
- *`level_min`: u8 (optional)* - The minimum zoom level to generate tiles for. (default: level_max)
- *`gdal_reuse_limit`: u32 (optional)* - How often to reuse a GDAL instance. (default: 100) Set to a lower value if you have problems like memory leaks in GDAL.
- *`gdal_concurrency_limit`: u8 (optional)* - The number of maximum concurrent GDAL instances to allow. (default: 4) Set to a higher value if you have enough system resources and want to increase throughput.
- *`cutline`: String (optional)* - Optional path to a GeoJSON file with Polygon/MultiPolygon geometry. Only pixels inside the polygon will be rendered; everything outside becomes nodata.

---

## from_gdal_raster

Reads a GDAL raster dataset and exposes it as a tile source.
Hint: When using "gdalbuildvrt" to create a virtual raster, don't forget to set `-addalpha` option to include alpha channel.

### Parameters

- **`filename`: String (required)** - The filename of the GDAL raster dataset to read. For example: `filename="world.tif"`.
- *`tile_size`: u32 (optional)* - The size of the generated tiles in pixels. (default: 512)
- *`tile_format`: TileFormat (optional)* - The tile format to use for the output tiles. (default: `PNG`)
- *`level_max`: u8 (optional)* - The maximum zoom level to generate tiles for. (default: the maximum zoom level based on the dataset's native resolution)
- *`level_min`: u8 (optional)* - The minimum zoom level to generate tiles for. (default: level_max)
- *`gdal_reuse_limit`: u32 (optional)* - How often to reuse an GDAL instances. (default: 100) Set to a lower value if you have problems like memory leaks in GDAL.
- *`gdal_concurrency_limit`: u8 (optional)* - The number of maximum concurrent GDAL instances to allow. (default: 4) Set to a higher value if you have enough system resources and want to increase throughput.
- *`cutline`: String (optional)* - Optional path to a GeoJSON file with Polygon/MultiPolygon geometry. Only pixels inside the polygon will be rendered; everything outside becomes transparent.
- *`bands`: String (optional)* - Comma-separated list of 1-based band indices to use as color channels. E.g. "4,3,2" maps band 4→Red, band 3→Green, band 2→Blue. "1" maps band 1→Grey. Defaults to auto-detection from color interpretation.
- *`nodata`: String (optional)* - NoData value(s) to treat as transparent. Multiple values can be separated by semicolons (e.g. "0;255" treats both 0 and 255 as nodata). Each value can be a single number applied to all bands or comma-separated per-band values (e.g. "0,0,0;255,255,255"). The first value is handled natively by GDAL during reprojection; additional values are applied as a post-warp alpha mask. If not specified, the source dataset's per-band nodata value is used (if any).
- *`crs`: u32 (optional)* - Override the source CRS with an EPSG code (e.g. "4326" or "25832"). Use this when the input image has no embedded CRS or an incorrect one.

---

## from_geo

Reads a GeoJSON or Shapefile and emits MVT vector tiles.

### Parameters

- **`filename`: String (required)** - Filename of the input (relative to the VPL file path). Format is detected from the extension: - `.geojson` / `.json` — GeoJSON `FeatureCollection` - `.ndjson` / `.geojsonl` / `.ndgeojson` / `.geojsonseq` — line-delimited GeoJSON (one feature per line; `.geojsonseq` may use the RFC 8142 record-separator prefix `U+001E`) - `.shp` — Esri Shapefile
- *`layer_name`: String (optional)* - Name of the MVT layer in the output tiles. Defaults to the filename stem.
- *`min_zoom`: u8 (optional)* - Lowest zoom level emitted (default 0).
- *`max_zoom`: u8 (optional)* - Highest zoom level emitted. Defaults to an auto-heuristic (median feature size ≈ 4 tile-pixels, capped at 14).
- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding-box clip in degrees `[w, s, e, n]`. Not supported in v1; setting this errors out.
- *`properties_include`: [String,...] (optional)* - Property whitelist: keep only the named properties, drop everything else. Mutually exclusive with `properties_exclude`.
- *`properties_exclude`: [String,...] (optional)* - Property blacklist: drop the named properties, keep everything else. Mutually exclusive with `properties_include`.
- *`polygon_min_area`: f32 (optional)* - Drop polygons whose area is below this many tile-pixels² (default 4).
- *`polygon_simplify`: f32 (optional)* - Douglas-Peucker tolerance for polygons, in tile-pixels (default 4).
- *`line_min_length`: f32 (optional)* - Drop lines whose length is below this many tile-pixels (default 4).
- *`line_simplify`: f32 (optional)* - Douglas-Peucker tolerance for lines, in tile-pixels (default 4).
- *`point_reduction`: PointReductionStrategy (optional)* - Point reduction strategy: `none` / `drop_rate` / `min_distance` (default `min_distance`).
- *`point_reduction_value`: f32 (optional)* - Numeric value whose meaning depends on `point_reduction`: - `min_distance` (default): minimum distance between kept points, in tile-pixels at the current zoom. Defaults to 16. - `drop_rate`: per-zoom keep-fraction in `[0, 1]`. Defaults to 0.5. - `none`: ignored.
- *`compression`: TileCompression (optional)* - Tile-compression applied before the tiles leave this operation: `gzip` (default), `brotli`, `zstd`, or `none`.
- *`max_tile_bytes`: u32 | none (optional)* - Maximum encoded tile size in bytes before a tile is considered broken and dropped (streaming path) / errors out (single-tile path). Defaults to 1048576 (1 MiB). Raise it when a legitimate low-zoom tile exceeds the default (e.g. `max_tile_bytes=2097152` for 2 MiB), or set `max_tile_bytes=none` to emit tiles at any size. The soft-cap warning threshold (200 KB at the default cap) scales with this value.
- *`ignore_id`: bool (optional)* - If `true`, drop the GeoJSON / Shapefile `id` field from every feature before encoding. Useful for sources where the id is a string (e.g. USGS earthquakes — those would be silently dropped at MVT encode anyway, since MVT requires `uint64` ids), or when the id is just noise. Defaults to `false` — keep the id when it's a non-negative integer.

---

## from_grid

Generates vector tiles containing the cells of a projected square grid, ready to be joined with data keyed on the cell id.

### Parameters

- **`epsg`: u32 (required)** - EPSG code of the grid's coordinate reference system. Without GDAL: `3035` (ETRS89-LAEA, what European gridded statistics use), `3857` (web mercator) and `4326` (WGS84 lon/lat).
- **`size`: f64 (required)** - Edge length of a cell, in the CRS's own units — meters for a projected CRS, degrees for `epsg=4326`. For example: `size=1000` for the 1 km grid Eurostat publishes.
- **`bbox`: [f64,f64,f64,f64] (required)** - The area to cover, as `[west, south, east, north]` in WGS84 degrees. Required: an unbounded grid has no pyramid to derive, and at most cell sizes it is more tiles than can be written.
- *`offset`: [f64,f64] (optional)* - Where the cell with index `(0, 0)` has its lower-left corner, in CRS units. Default: `[0,0]`, which is what published grids align to.
- *`max_cells_per_tile`: u32 (optional)* - Roughly how many cells one tile may hold. Decides the lowest zoom level this source offers. Default: `1024`.
- *`max_zoom`: u8 (optional)* - Highest zoom level to generate. Defaults to three levels above the derived minimum, since further levels repeat the same cells.
- *`id_preset`: String (optional)* - Ready-made id format. Either `"inspire"` (default), which produces `CRS3035RES1000mN2691000E4341000`, or `"geostat"`, which produces `1kmN2689E4337`.
- *`id_template`: String (optional)* - Id format spelled out, for a grid whose publisher uses neither preset. `{x}` and `{y}` are the lower-left corner, each taking an optional divisor and zero-padded width: `E{x/100:04}N{y/100:04}` produces `E0643N4567`, the form Dutch grid statistics use. Overrides `id_preset`.
- *`id_field`: String (optional)* - Name of the string property holding the cell id. Default: `"id"`.
- *`x_field`: String (optional)* - Names of the number properties holding the lower-left corner. Defaults: `"x"` and `"y"`, mirroring the `X_LLC` / `Y_LLC` columns Eurostat ships beside its ids.
- *`y_field`: String (optional)* - See `x_field`.
- *`densify_tolerance`: f64 (optional)* - How far a cell edge may stray from its true curve, in tile pixels. Only matters for a CRS whose straight lines bend in mercator; in `3857` and `4326` no vertices are added whatever this says. Default: `0.5`.
- *`layer_name`: String (optional)* - Name of the layer in the generated tiles. Default: `"grid"`.

---

## from_h3

Generates vector tiles containing H3 grid cells, ready to be joined with data keyed on the H3 index.

### Parameters

- **`resolution`: u8 (required)** - H3 resolution, `0` (cells of ~4,250,000 km²) to `15` (~0.9 m²). For example: `resolution=8` for cells of roughly 0.7 km². See <https://h3geo.org/docs/core-library/restable/> for the full table.
- **`bbox`: [f64,f64,f64,f64] (required)** - The area to cover, as `[west, south, east, north]` in WGS84 degrees. For example: `bbox=[13.0,52.3,13.8,52.7]` for Berlin. Required: a grid without bounds would be generated for the whole planet, which at most resolutions is more tiles than can be written.
- *`max_cells_per_tile`: u32 (optional)* - Roughly how many cells one tile may hold. Decides the lowest zoom level this source offers: below it a single tile would carry more cells than a tile can usefully hold. Default: `1024`.
- *`max_zoom`: u8 (optional)* - Highest zoom level to generate. Defaults to three levels above the derived minimum, since further levels repeat the same cells.
- *`layer_name`: String (optional)* - Name of the layer in the generated tiles. Default: `"grid"`.
- *`id_field`: String (optional)* - Name of the string property holding the H3 index, e.g. `"8928308280fffff"`. Default: `"h3"`, matching how H3 datasets usually name the column.

---

## from_merged_vector

Merges multiple vector tile sources.
Each resulting tile will contain all the features and properties from all the sources.

### Sources

All tile sources must provide vector tiles.

---

## from_stacked

Overlays multiple tile sources, using the tile from the first source that provides it.

### Sources

All tile sources must have the same format.

---

## from_stacked_raster

Overlays multiple raster tile sources on top of each other.

### Sources

All tile sources must provide raster tiles in the same resolution. The first source overlays the others.

### Parameters

- *`format`: TileFormat (optional)* - The tile format to use for the output tiles. Default: format of the first source.
- *`auto_overscale`: bool (optional)* - Whether to automatically wrap each source with `raster_overscale` so that sources missing native tiles at the requested zoom level still contribute via upscaled tiles. When all sources overlapping a requested bbox are overscaled (none have native data), this operation returns an empty stream. Place a `raster_overscale` *after* `from_stacked_raster` in the pipeline to cover those tiles — it is more efficient to upscale one blended tile than N individual tiles. Default: `false`.

---

## from_tile

Reads a single tile file and uses it as a template for all tile requests.

### Parameters

- **`filename`: String (required)** - The filename of the tile. Supported formats: png, jpg/jpeg, webp, avif, pbf/mvt. The format is automatically detected from the file extension.

---

## from_tilejson

Reads tiles from a remote tile server via a TileJSON endpoint.
The TileJSON is fetched from the given URL, and tiles are loaded individually
using the URL template from the TileJSON `tiles` array.

### Parameters

- **`url`: String (required)** - The URL of the TileJSON endpoint. For example: `url="https://example.com/tiles.json"`.
- *`max_retries`: u16 (optional)* - Maximum number of retries per tile request (default: 3).
- *`max_concurrent_requests`: u16 (optional)* - Maximum number of concurrent tile requests (default: io_bound concurrency limit).

---

# TRANSFORM operations

---

## dem_overview

Generate lower-zoom DEM overview tiles by averaging 24-bit elevation values.

Unlike raster_overview which averages RGB channels independently,
this operation decodes each pixel to its 24-bit raw elevation value,
averages the values correctly, and re-encodes back to RGB.

### Parameters

- *`level`: u8 (optional)* - Use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
- *`encoding`: String (optional)* - Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".

---

## dem_quantize

Quantize DEM (elevation) raster tiles by rounding to a per-tile power-of-two step.

Computes the step from two physically meaningful criteria: elevation error relative to
pixel size, and maximum slope distortion. The stricter (smaller step) wins. Values are
rounded to the nearest multiple of the step (not truncated), which halves the worst-case
elevation error and removes the downward bias at no size cost. Single-pass — no scan.

### Parameters

- *`elevation_error`: f64 (optional)* - Allowed elevation error as fraction of pixel ground size. E.g. 0.1 means for a 10 m pixel, allow up to 1 m elevation error. Defaults to 0.1.
- *`slope_error`: f64 (optional)* - Maximum allowed slope change in degrees due to quantization. Defaults to 1.0.
- *`encoding`: String (optional)* - Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".

---

## dem_tile_resize

Convert DEM tile size between 256px and 512px by splitting or merging tiles.

Like raster_tile_resize, but uses 24-bit raw value averaging for downscaling
(level 0, 512→256) instead of channel-wise averaging.

### Parameters

- *`tile_size`: u32 (optional)* - Target tile size in pixels. Must be 256 or 512.
- *`encoding`: String (optional)* - Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".

---

## filter

Filter tiles by bounding box, zoom levels, and/or the tile coordinates present in another container.

Every parameter narrows the tile set, except `bbox_border`, which widens it
by keeping a ring of tiles around `bbox`.

### Parameters

- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding box in WGS84: [min lng, min lat, max lng, max lat].
- *`bbox_border`: u32 (optional)* - Ring of extra tiles to keep around `bbox`, in tiles per zoom level. Note that this is the one parameter here that *widens*: every other parameter narrows the tile set, but `bbox_border=2` keeps tiles the bbox alone would have dropped. Those tiles lie outside the crop, so the advertised bounds are extended to cover them and a client actually requests them. This matters wherever a cropped tileset is rendered rather than just stored: without a border, labels and geometry near the edge have no neighbouring tiles to be laid out against. Requires `bbox`; setting it alone is an error rather than a no-op.
- *`level_min`: u8 (optional)* - minimal zoom level
- *`level_max`: u8 (optional)* - maximal zoom level
- *`filename`: String (optional)* - Path to a tile container used as a coordinate allow-list. Only tiles whose coordinates exist in this container are passed through. Accepts the same path/URL syntax as `from_container`. Note: opening the container and building the allow-list requires I/O at pipeline build time.

---

## meta_update

Update metadata, see also <https://github.com/mapbox/tilejson-spec/tree/master/3.0.0>

### Parameters

- *`attribution`: String (optional)* - Attribution text.
- *`bounds`: [f64,f64,f64,f64] (optional)* - Geographic bounding box [west, south, east, north].
- *`center`: [f64,f64,f64] (optional)* - Default center [longitude, latitude, zoom].
- *`description`: String (optional)* - Description text.
- *`fillzoom`: u8 (optional)* - Fill zoom level.
- *`legend`: String (optional)* - Legend text.
- *`name`: String (optional)* - Name text.
- *`schema`: TileSchema (optional)* - Tile schema, allowed values: "rgb", "rgba", "dem/mapbox", "dem/terrarium", "dem/versatiles", "openmaptiles", "shortbread@1.0", "other", "unknown"
- *`tilejson`: String (optional)* - A complete TileJSON document (JSON string) used as the basis for the new metadata. When given, the new metadata starts from this document instead of the source's; the other parameters then override individual fields on top of it.
- *`tilejson_file`: String (optional)* - Path to a file containing a complete TileJSON document, resolved relative to the VPL file. Use instead of `tilejson` to avoid inline JSON quoting. Mutually exclusive with `tilejson`.
- *`tilejson_update`: String (optional)* - A partial TileJSON document (JSON string) merged onto the current metadata. Scalar fields (e.g. `name`, `attribution`) and `vector_layers` overwrite; `bounds` and the zoom range are widened to the union. The individual parameters still take precedence.
- *`tilejson_update_file`: String (optional)* - Path to a file containing a partial TileJSON document, resolved relative to the VPL file. Use instead of `tilejson_update`. Mutually exclusive with `tilejson_update`.
- *`vector_layers`: String (optional)* - The `vector_layers` array as a JSON string. It is parsed and validated against the TileJSON spec before replacing the source's `vector_layers`.
- *`vector_layers_file`: String (optional)* - Path to a file containing the `vector_layers` array as JSON, resolved relative to the VPL file. Use instead of `vector_layers`. Mutually exclusive with `vector_layers`.

---

## raster_flatten

Flattens (translucent) raster tiles onto a background

### Parameters

- *`color`: [u8,u8,u8] (optional)* - background color to use for the flattened tiles, in RGB format. Defaults to white.

---

## raster_format

Convert raster tiles to a different image format and/or adjust quality/effort settings.

### Parameters

- *`format`: RasterTileFormat (optional)* - The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP. If not specified, the source format will be used.
- *`quality`: String (optional)* - Quality level for the tile compression (only AVIF, JPG or WEBP), between 0 (worst) and 100 (lossless). To allow different quality levels for different zoom levels, this can also be a comma-separated list like this: "70,14:50,15:20", where the first value is the default quality, and the other values specify the quality for the specified zoom level (and higher).
- *`quality_translucent`: String (optional)* - Quality level for translucent (semi-transparent) tiles, using the same zoom-dependent syntax as quality. When set, tiles are checked for opacity: opaque tiles use the normal quality setting, while translucent tiles use this value (typically 100 for lossless).
- *`effort`: u8 (optional)* - Compression effort, between 0 (fastest) and 100 (slowest/best).

---

## raster_levels

Adjust brightness, contrast and gamma of raster tiles.

### Parameters

- *`brightness`: f32 (optional)* - Brightness adjustment, between -255 and 255. Defaults to 0.0 (no change).
- *`contrast`: f32 (optional)* - Contrast adjustment, between 0 and infinity. Defaults to 1.0 (no change).
- *`gamma`: f32 (optional)* - Gamma adjustment, between 0 and infinity. Defaults to 1.0 (no change).

---

## raster_mask

Apply a polygon mask from GeoJSON to raster tiles.
Pixels outside the polygon become transparent.

### Parameters

- **`geojson`: String (required)** - Path to GeoJSON file with Polygon or MultiPolygon geometry.
- *`buffer`: f32 (optional)* - Buffer distance in meters. Positive values expand the mask, negative values shrink it. Default: 0
- *`blur`: f32 (optional)* - Edge blur distance in meters. Creates a soft transition at the mask edge. Default: 0
- *`blur_function`: String (optional)* - Blur falloff function: "linear" or "cosine". Default: "linear"

---

## raster_overscale

Raster overscale operation - generates tiles beyond the source's native resolution.

### Parameters

- *`level_base`: u8 (optional)* - The zoom level to use as the source for overscaling. Tiles at this level and below are passed through unchanged. Tiles above this level are generated by extracting and upscaling from this level. Defaults to the maximum zoom level of the source.
- *`level_max`: u8 (optional)* - The maximum zoom level to support. Defaults to 30. Requests above this level will not return tiles.
- *`enable_climbing`: bool (optional)* - Enable tile climbing when the expected source tile doesn't exist. When true, the operation will search parent tiles at lower zoom levels until it finds an existing tile, then extract and upscale from there. Defaults to false.

---

## raster_overview

Generate lower-zoom overview tiles by downscaling from a base zoom level.

### Parameters

- *`level`: u8 (optional)* - use this zoom level to build the overview. Defaults to the maximum zoom level of the source.

---

## raster_tile_resize

Convert the size of tiles by splitting or merging them to a width of 256px or 512px.

### Parameters

- *`tile_size`: u32 (optional)* - Target tile size in pixels. A value of `256` expects source tiles of 512px, which will be split into four 256px output tiles at the next higher zoom level. Level 0 is downscaled instead. A value of `512` expects source tiles measuring 256px, which will be merged into 512px output tiles at the next lower zoom level.

---

## remap_coords

Relabels tile coordinates, e.g. to correct a source that uses TMS row order or `z/y/x` paths.

The three flags are applied in a fixed order — `flip_x`, then `flip_y`, then
`swap_xy` — and between them reach all eight symmetries of the square: four
rotations and four reflections, which is every relabelling that maps the tile
grid onto itself. Because that set is closed, chaining two of these
operations is never necessary; some single combination of the three flags
does the same thing.

The combinations most likely to be wanted:

| `flip_x` | `flip_y` | `swap_xy` | result                    |
| -------- | -------- | --------- | ------------------------- |
| false    | true     | false     | TMS ↔ XYZ row order       |
| false    | false    | true      | `z/y/x` ↔ `z/x/y` layouts |
| true     | true     | false     | rotate 180°               |
| true     | false    | true      | rotate 90°                |

Unlike a global `--flip-y` flag, this applies to one source, so a pipeline
can combine sources that disagree about their conventions.

### Parameters

- *`flip_x`: bool (optional)* - Mirror horizontally within each zoom level: `x` becomes `2^z - 1 - x`. No tile scheme uses this on its own; it is what makes the rotations reachable. Defaults to `false`.
- *`flip_y`: bool (optional)* - Mirror vertically within each zoom level: `y` becomes `2^z - 1 - y`. This is the TMS ↔ XYZ correction. Defaults to `false`.
- *`swap_xy`: bool (optional)* - Exchange the axes: `(x, y)` becomes `(y, x)`, for sources laid out as `z/y/x`. Applied after the flips. Defaults to `false`.

---

## vector_filter_features

Drops vector features in selected layers that do not satisfy a boolean CEL expression.
Features in layers outside `layer` pass through untouched.

### Examples

```text
vector_filter_features layer=["place"] expr="name == 'Berlin'"
vector_filter_features layer=["poi"]   expr="population >= 1000"
vector_filter_features layer=["road"]  expr="highway in ['primary','secondary']"
vector_filter_features layer=["place"] expr="name.matches('^St\\.')"
vector_filter_features layer=["poi"]   expr="name != null && name != ''"
vector_filter_features layer=["addr"]  expr="props['addr:street'] == 'Hauptstr.'"
```

### Parameters

- **`layer`: [String,...] (required)** - Layers the expression applies to, as a VPL array of strings. Features in all other layers are left unchanged. Example: `layer=["poi","place"]`.
- **`expr`: String (required)** - CEL (Common Expression Language) boolean expression. Feature properties are available as `props["key"]`; properties whose names are valid CEL identifiers (letters, digits, underscore) are also exposed as top-level identifiers. Missing keys resolve to null; use `name != null` (for identifier-safe keys) or `has(props.key)` (for any key) for explicit presence checks. See `versatiles help` for a CEL operator cheat-sheet.

---

## vector_filter_layers

Filters vector tile layers by name.

### Parameters

- **`filter`: [String,...] (required)** - Layer names to remove from the tiles, e.g. `filter=["pois","ocean"]`.
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only layers matching the filter).

---

## vector_filter_properties

Filters properties based on a regular expressions.

### Parameters

- **`regex`: String (required)** - A regular expression pattern that should match property names to be removed from all features. The property names contain the layer name as a prefix, e.g., `layer_name/property_name`, so an expression like `regex="^layer_name/"` will match all properties of that layer or `regex="/name_.*$"` will match all properties starting with `name_` in all layers.
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only properties matching the filter).

---

## vector_overzoom

Vector overzoom operation - generates vector tiles beyond the source's native max zoom.

### Parameters

- *`level_base`: u8 (optional)* - The zoom level to use as the source for overzooming. Tiles at this level and below are passed through unchanged. Tiles above this level are generated by clipping and rescaling features from the corresponding parent tile at this level. Defaults to the maximum zoom level of the source.
- *`level_max`: u8 (optional)* - The maximum zoom level to support. Defaults to `level_base + 4` (each extra level quadruples the tile count, so 4 levels = 256× — usually the sweet spot before the pyramid becomes unwieldy). Set explicitly if you want to overzoom further. Capped at 30.
- *`enable_climbing`: bool (optional)* - Enable tile climbing when the expected source tile doesn't exist. When true, the operation will search parent tiles at lower zoom levels until it finds an existing tile, then clip and rescale from there. Defaults to false.
- *`buffer`: u32 (optional)* - Clip buffer in tile-extent units, applied to the child tile's sub-region so that features straddling tile boundaries (labels, lines) survive intact. Defaults to 80.

---

## vector_repair

Repairs vector tiles to conform to MVT 2.1.

Always fixed: missing `extent`/`version` fields, duplicate layer names,
inverted polygon winding, and degenerate rings.

Tiles that the validator considers clean pass through unchanged — the
original encoded blob is forwarded without re-encoding, so this operation
is cheap on conformant input.

### Arguments

- `drop_offenders` (bool, default `false`): when `true`, features whose
geometry byte stream cannot be decoded are silently removed. When `false`
(the default), any layer containing such features keeps its original
geometry bytes intact while structural fixes (extent, version) are still
applied.

### Example

```text
from_container filename="bad.versatiles" | vector_repair
from_container filename="bad.versatiles" | vector_repair drop_offenders=true
```

### Parameters

- *`drop_offenders`: bool (optional)* - Drop features that cannot be decoded rather than leaving them in place. Defaults to false.

---

## vector_update_properties

Arguments for the `vector_update_properties` operation.

This operation joins vector tile features with external tabular data (CSV/TSV)
based on matching ID fields, allowing you to enrich or update feature properties.

### Parameters

- **`data_source_path`: String (required)** - Path to the CSV/TSV data file: The file must have a header row. Each subsequent row will be matched to vector features using the ID fields.
- **`layer_name`: String (required)** - Name of the vector layer to update: Only features in this layer will be modified. Other layers pass through unchanged.
- **`id_field_tiles`: String (required)** - Field name in the vector tiles that contains the feature ID: This field is used to match features with rows in the data source.
- **`id_field_data`: String (required)** - Column name in the data source that contains the matching ID: This column is used to look up data for each feature.
- *`replace_properties`: bool (optional)* - If `true`, replaces all existing properties with the data source values. If `false` (default), merges new properties with existing ones.
- *`remove_non_matching`: bool (optional)* - If `true`, removes features that don't have a matching row in the data source. If `false` (default), non-matching features are kept unchanged.
- *`include_id`: bool (optional)* - If `true`, includes the ID field from the data source in the output properties. If `false` (default), the ID field is excluded from the merged properties.
- *`field_separator`: String (optional)* - Field separator character for the data file: Default for `.csv` files is `,` (comma). Default for `.tsv` files is `\t` (tab, auto-detected)
- *`decimal_separator`: String (optional)* - Decimal separator character for parsing numbers: Default is `.` (US/UK format). Use `,` (comma) e.g. for German/European number format like `1.234,56`
