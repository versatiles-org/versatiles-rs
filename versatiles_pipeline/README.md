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

---

# READ operations

## `from_color`

Generates solid-color tiles of the specified size and format.

### Parameters

- *`color`: String (optional)* - Hex color in RGB or RGBA format (e.g., "FF5733" or "FF573380"). Defaults to "000000" (black).
- *`size`: u16 (optional)* - Tile size in pixels (256 or 512). Defaults to 512.
- *`format`: String (optional)* - Tile format: one of "avif", "jpg", "png", or "webp". Defaults to "png".

## `from_container`

Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.

### Parameters

- **`filename`: String (required)** - The filename of the tile container. This is relative to the path of the VPL file. For example: `filename="world.versatiles"`.

## `from_debug`

Generates debug tiles that display their coordinates as text.

### Parameters

- *`format`: String (optional)* - Target tile format: one of `"mvt"` (default), `"avif"`, `"jpg"`, `"png"` or `"webp"`

## `from_gdal_raster`

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

## `from_merged_vector`

Merges multiple vector tile sources.
Each resulting tile will contain all the features and properties from all the sources.

### Sources

All tile sources must provide vector tiles.

## `from_stacked`

Overlays multiple tile sources, using the tile from the first source that provides it.

### Sources

All tile sources must have the same format.

## `from_stacked_raster`

Overlays multiple raster tile sources on top of each other.

### Sources

All tile sources must provide raster tiles in the same resolution. The first source overlays the others.

### Parameters

- *`format`: TileFormat (optional)* - The tile format to use for the output tiles. Default: format of the first source.
- *`auto_overscale`: bool (optional)* - Whether to automatically overscale tiles when a source does not provide tiles at the requested zoom level. Default: `false`.

## `from_tile`

Reads a single tile file and uses it as a template for all tile requests.

### Parameters

- **`filename`: String (required)** - The filename of the tile. Supported formats: png, jpg/jpeg, webp, avif, pbf/mvt. The format is automatically detected from the file extension.

## `from_tilejson`

Reads tiles from a remote tile server via a TileJSON endpoint.
The TileJSON is fetched from the given URL, and tiles are loaded individually
using the URL template from the TileJSON `tiles` array.

### Parameters

- **`url`: String (required)** - The URL of the TileJSON endpoint. For example: `url="https://example.com/tiles.json"`.
- *`max_retries`: u16 (optional)* - Maximum number of retries per tile request (default: 3).
- *`max_concurrent_requests`: u16 (optional)* - Maximum number of concurrent tile requests (default: io_bound concurrency limit).

---

# TRANSFORM operations

## `dem_quantize`

Quantize DEM (elevation) raster tiles by zeroing unnecessary low bits.
Computes a per-tile quantization mask from two physically meaningful criteria:
resolution relative to tile size, and maximum gradient distortion.
The stricter (smaller step) wins. Single-pass — no min/max scan needed.

### Parameters

- *`resolution_ratio`: f64 (optional)* - Minimum elevation resolution as fraction of tile ground size. E.g. 0.001 means for a 1000 m tile, keep 1 m resolution. Defaults to 0.001.
- *`max_gradient_error`: f64 (optional)* - Maximum allowed gradient change in degrees due to quantization. Defaults to 1.0.
- *`encoding`: String (optional)* - Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".

## `filter`

Filter tiles by bounding box and/or zoom levels.

### Parameters

- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding box in WGS84: [min lng, min lat, max lng, max lat].
- *`level_min`: u8 (optional)* - minimal zoom level
- *`level_max`: u8 (optional)* - maximal zoom level

## `meta_update`

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

## `raster_flatten`

Flattens (translucent) raster tiles onto a background

### Parameters

- *`color`: [u8,u8,u8] (optional)* - background color to use for the flattened tiles, in RGB format. Defaults to white.

## `raster_format`

Filter tiles by bounding box and/or zoom levels.

### Parameters

- *`format`: String (optional)* - The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP. If not specified, the source format will be used.
- *`quality`: String (optional)* - Quality level for the tile compression (only AVIF, JPG or WEBP), between 0 (worst) and 100 (lossless). To allow different quality levels for different zoom levels, this can also be a comma-separated list like this: "80,70,14:50,15:20", where the first value is the default quality, and the other values specify the quality for the specified zoom level (and higher).
- *`speed`: u8 (optional)* - Compression speed (only AVIF), between 0 (slowest) and 100 (fastest).

## `raster_levels`

Adjust brightness, contrast and gamma of raster tiles.

### Parameters

- *`brightness`: f32 (optional)* - Brightness adjustment, between -255 and 255. Defaults to 0.0 (no change).
- *`contrast`: f32 (optional)* - Contrast adjustment, between 0 and infinity. Defaults to 1.0 (no change).
- *`gamma`: f32 (optional)* - Gamma adjustment, between 0 and infinity. Defaults to 1.0 (no change).

## `raster_mask`

Apply a polygon mask from GeoJSON to raster tiles.
Pixels outside the polygon become transparent.

### Parameters

- **`geojson`: String (required)** - Path to GeoJSON file with Polygon or MultiPolygon geometry.
- *`buffer`: f32 (optional)* - Buffer distance in meters. Positive values expand the mask, negative values shrink it. Default: 0
- *`blur`: f32 (optional)* - Edge blur distance in meters. Creates a soft transition at the mask edge. Default: 0
- *`blur_function`: String (optional)* - Blur falloff function: "linear" or "cosine". Default: "linear"

## `raster_overscale`

Raster overscale operation - generates tiles beyond the source's native resolution.

### Parameters

- *`level_base`: u8 (optional)* - The zoom level to use as the source for overscaling. Tiles at this level and below are passed through unchanged. Tiles above this level are generated by extracting and upscaling from this level. Defaults to the maximum zoom level of the source.
- *`level_max`: u8 (optional)* - The maximum zoom level to support. Defaults to 30. Requests above this level will not return tiles.
- *`enable_climbing`: bool (optional)* - Enable tile climbing when the expected source tile doesn't exist. When true, the operation will search parent tiles at lower zoom levels until it finds an existing tile, then extract and upscale from there. Defaults to false.

## `raster_overview`

Filter tiles by bounding box and/or zoom levels.

### Parameters

- *`level`: u8 (optional)* - use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
- *`tile_size`: u32 (optional)* - Size of the tiles in pixels. Defaults to 512.

## `vector_filter_layers`

Filters vector tile layers based on a comma-separated list of layer names.

### Parameters

- **`filter`: String (required)** - Comma‑separated list of layer names that should be removed from the tiles, e.g.: filter="pois,ocean".
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only layers matching the filter).

## `vector_filter_properties`

Filters properties based on a regular expressions.

### Parameters

- **`regex`: String (required)** - A regular expression pattern that should match property names to be removed from all features. The property names contain the layer name as a prefix, e.g., `layer_name/property_name`, so an expression like `regex="^layer_name/"` will match all properties of that layer or `regex="/name_.*$"` will match all properties starting with `name_` in all layers.
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only properties matching the filter).

## `vector_update_properties`

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
