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

---

# READ operations

---

## from_color

Generates solid-color tiles of the specified size and format.

### Parameters

- *`color`: String (optional)* - Hex color in RGB or RGBA format (e.g., "FF5733" or "FF573380"). Defaults to "000000" (black).
- *`size`: u16 (optional)* - Tile size in pixels (256 or 512). Defaults to 512.
- *`format`: String (optional)* - Tile format: one of "avif", "jpg", "png", or "webp". Defaults to "png".

---

## from_container

Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.

### Parameters

- **`filename`: String (required)** - The filename of the tile container (relative to the VPL file path), or a URL (http/https). For example: `filename="world.versatiles"` or `filename="https://example.com/world.versatiles"`.

---

## from_debug

Generates debug tiles that display their coordinates as text.

### Parameters

- *`format`: String (optional)* - Target tile format: one of `"mvt"` (default), `"avif"`, `"jpg"`, `"png"` or `"webp"`

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

Quantize DEM (elevation) raster tiles by zeroing unnecessary low bits.

Computes a per-tile quantization mask from two physically meaningful criteria:
elevation error relative to pixel size, and maximum slope distortion.
The stricter (smaller step) wins. Single-pass — no min/max scan needed.

### Parameters

- *`elevation_error`: f64 (optional)* - Allowed elevation error as fraction of pixel ground size. E.g. 0.5 means for a 10 m pixel, allow up to 5 m elevation error. Defaults to 0.5.
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

### Parameters

- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding box in WGS84: [min lng, min lat, max lng, max lat].
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

---

## raster_flatten

Flattens (translucent) raster tiles onto a background

### Parameters

- *`color`: [u8,u8,u8] (optional)* - background color to use for the flattened tiles, in RGB format. Defaults to white.

---

## raster_format

Convert raster tiles to a different image format and/or adjust quality/effort settings.

### Parameters

- *`format`: String (optional)* - The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP. If not specified, the source format will be used.
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
