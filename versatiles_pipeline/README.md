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

---

# READ operations

---

## from_color

Generates raster tiles of a single solid colour.

### Parameters

- _`color`: String (optional)_ - Hex colour, `RRGGBB` or `RRGGBBAA`. Defaults to `000000`.
- _`size`: u16 (optional)_ - Tile size in pixels, `256` or `512`. Defaults to `512`.
- _`format`: RasterTileFormat (optional)_ - Values: `avif`, `jpg`, `png`, `webp`. Format to encode the tiles in. Defaults to `png`.

---

## from_container

Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.

`filename` takes a local path or a URL — `filename="world.versatiles"` as readily as `filename="https://example.com/world.versatiles"`. Run `versatiles help source` for the URL and authentication syntax.

`ssh_identity` names a key for this source alone, overriding `--ssh-identity` and `VERSATILES_SSH_IDENTITY`, so one pipeline can read from two SFTP hosts that need different keys. It is ignored for every other scheme, and naming a key ties the `.vpl` file to machines that have it.

### Parameters

- **`filename`: String (required)** - Path to the container, or an `http`, `https` or `sftp` URL.
- _`ssh_identity`: String (optional)_ - Private key for this one `sftp://` source. Defaults to the global setting.

---

## from_csv

Reads a CSV file with longitude and latitude columns and emits MVT point tiles.

Left to itself, `max_zoom` picks the level at which the median feature spans roughly 4 tile-pixels, capped at 14. Points count as size zero, so a CSV of points always lands on the cap.

`properties_include` and `properties_exclude` act on what is left after the coordinate and id columns are consumed, so naming `lon_column`, `lat_column` or `id_column` in either has no effect.

A tile over `max_tile_bytes` is dropped while streaming and an error when a single tile is requested. Raise the cap when a legitimate low-zoom tile exceeds it, or set `max_tile_bytes=none` to emit tiles at any size; the soft-cap warning threshold, 200 KB at the default, scales with it.

`bbox` is `[west, south, east, north]` in degrees, and crops while reading: rows outside it are dropped before anything is built, and the tile pyramid is clipped to it. That turns a large CSV into one region's tiles in a single pass, where cropping afterwards with `versatiles convert --bbox` means writing the whole thing out first. The crop is tile-granular, as it is there.

### Parameters

- **`filename`: String (required)** - Path to the CSV file.
- **`lon_column`: String (required)** - Column holding the longitude, in WGS84 degrees.
- **`lat_column`: String (required)** - Column holding the latitude, in WGS84 degrees.
- _`id_column`: String (optional)_ - Column to expose as the feature id. Defaults to emitting no id.
- _`delimiter`: String (optional)_ - Character separating a row's fields. Defaults to `,`.
- _`has_header`: bool (optional)_ - Whether the first row holds column names; `false` is not supported yet. Defaults to `true`.
- _`layer_name`: String (optional)_ - Name of the layer to write into. Defaults to the file's stem.
- _`min_zoom`: u8 (optional)_ - Lowest zoom level to emit. Defaults to `0`.
- _`max_zoom`: u8 (optional)_ - Highest zoom level to emit. Defaults to a heuristic capped at `14`.
- _`bbox`: [f64,f64,f64,f64] (optional)_ - Area to restrict the output to, in WGS84 degrees. Defaults to the input's extent.
- _`properties_include`: [String,...] (optional)_ - Columns to keep as properties. Mutually exclusive with `properties_exclude`. Defaults to all.
- _`properties_exclude`: [String,...] (optional)_ - Columns to drop. Mutually exclusive with `properties_include`. Defaults to none.
- _`point_reduction`: PointReductionStrategy (optional)_ - Values: `none`, `drop_rate`, `min_distance`. How to thin out points too close to distinguish. Defaults to `min_distance`.
- _`point_reduction_value`: f32 (optional)_ - Distance in tile-pixels for `min_distance`, keep-fraction for `drop_rate`. Defaults to `16`/`0.5`.
- _`compression`: TileCompression (optional)_ - Values: `none`, `gzip`, `brotli`, `zstd`. Compression applied before the tiles leave. Defaults to `gzip`.
- _`max_tile_bytes`: u32 | none (optional)_ - Size in bytes above which a tile counts as broken. Defaults to `1048576`.

---

## from_debug

Generates tiles that draw their own coordinates, for inspecting a pipeline.

### Parameters

- _`format`: DebugTileFormat (optional)_ - Values: `mvt`, `avif`, `jpg`, `png`, `webp`. Format to generate the tiles in. Defaults to `mvt`.

---

## from_gdal_dem

Reads a GDAL elevation dataset and encodes it as terrain RGB tiles.

### Parameters

- **`filename`: String (required)** - Path to the DEM dataset.
- _`encoding`: DemEncoding (optional)_ - Values: `mapbox`, `terrarium`. How elevation is packed into the RGB channels. Defaults to `mapbox`.
- _`tile_size`: u32 (optional)_ - Tile size in pixels. Defaults to `512`.
- _`level_max`: u8 (optional)_ - Highest zoom level to generate. Defaults to the dataset's native resolution.
- _`level_min`: u8 (optional)_ - Lowest zoom level to generate. Defaults to `level_max`.
- _`gdal_reuse_limit`: u32 (optional)_ - How many tiles a GDAL instance renders before being replaced. Defaults to `100`.
- _`gdal_concurrency_limit`: u8 (optional)_ - How many GDAL instances may run at once. Defaults to `4`.
- _`cutline`: String (optional)_ - GeoJSON polygon outside which pixels become nodata. Defaults to the whole dataset.

---

## from_gdal_raster

Reads a GDAL raster dataset and reprojects it into tiles.

When building a virtual raster with `gdalbuildvrt`, pass `-addalpha` — without it the VRT carries no alpha channel and nothing outside the data becomes transparent.

### Parameters

- **`filename`: String (required)** - Path to the raster dataset.
- _`tile_size`: u32 (optional)_ - Tile size in pixels. Defaults to `512`.
- _`level_max`: u8 (optional)_ - Highest zoom level to generate. Defaults to the dataset's native resolution.
- _`level_min`: u8 (optional)_ - Lowest zoom level to generate. Defaults to `level_max`.
- _`gdal_reuse_limit`: u32 (optional)_ - How many tiles a GDAL instance renders before being replaced. Defaults to `100`.
- _`gdal_concurrency_limit`: u8 (optional)_ - How many GDAL instances may run at once. Defaults to `4`.
- _`tile_format`: RasterTileFormat (optional)_ - Values: `avif`, `jpg`, `png`, `webp`. Format to encode the tiles in. Defaults to `png`.
- _`cutline`: String (optional)_ - GeoJSON polygon outside which pixels become transparent. Defaults to the whole dataset.
- _`bands`: String (optional)_ - Band indices to read as colour channels, 1-based. Defaults to the colour interpretation.
- _`nodata`: String (optional)_ - Pixel values to render as transparent. Defaults to the dataset's own.
- _`crs`: u32 (optional)_ - EPSG code to read the dataset with. Defaults to the dataset's own.

---

## from_geo

Reads a GeoJSON or Shapefile and emits MVT vector tiles.

The input format is detected from the file extension:

| Extension                                           | Format                                       |
| --------------------------------------------------- | -------------------------------------------- |
| `.geojson`, `.json`                                 | GeoJSON `FeatureCollection`                  |
| `.ndjson`, `.geojsonl`, `.ndgeojson`, `.geojsonseq` | line-delimited GeoJSON, one feature per line |
| `.shp`                                              | Esri Shapefile                               |

A `.geojsonseq` file may prefix each record with the RFC 8142 record separator `U+001E`.

Input must be **EPSG:4326 lon/lat in degrees, longitude first** — reprojection is not performed. Coordinates outside that range are refused, as is a GeoJSON `crs` member naming another projection, but lat/lon input with the axes swapped is in range and cannot be detected: it produces valid tiles of the wrong place. Reproject first if needed, e.g. `ogr2ogr -t_srs EPSG:4326 out.geojson in.geojson`. A shapefile without a `.prj` is read as WGS84 with a warning, since it carries no CRS of its own.

Left to itself, `max_zoom` picks the level at which the median feature spans roughly 4 tile-pixels, capped at 14. Points count as size zero, so a mostly-point dataset lands on the cap.

A tile over `max_tile_bytes` is dropped while streaming and an error when a single tile is requested. Raise the cap when a legitimate low-zoom tile exceeds it, or set `max_tile_bytes=none` to emit tiles at any size; the soft-cap warning threshold, 200 KB at the default, scales with it.

`bbox` is `[west, south, east, north]` in degrees, and crops while reading: features outside it are dropped before anything is built, and the tile pyramid is clipped to it. That turns a planet-scale extract into one city's tiles in a single pass, where cropping afterwards with `versatiles convert --bbox` means writing the whole thing out first. The crop is tile-granular, as it is there — a boundary tile keeps whatever falls inside it, including the parts of a feature that reach past the box.

`ignore_id` exists because MVT requires a `uint64` feature id. A string id — as USGS earthquake data has — is dropped at encode time anyway, so setting this makes that explicit; it is also the way to discard an id that is just noise.

### Parameters

- **`filename`: String (required)** - Path to the input file; its format comes from the extension.
- _`layer_name`: String (optional)_ - Name of the layer to write into. Defaults to the file's stem.
- _`min_zoom`: u8 (optional)_ - Lowest zoom level to emit. Defaults to `0`.
- _`max_zoom`: u8 (optional)_ - Highest zoom level to emit. Defaults to a heuristic capped at `14`.
- _`bbox`: [f64,f64,f64,f64] (optional)_ - Area to restrict the output to, in WGS84 degrees. Defaults to the input's extent.
- _`properties_include`: [String,...] (optional)_ - Properties to keep. Mutually exclusive with `properties_exclude`. Defaults to all.
- _`properties_exclude`: [String,...] (optional)_ - Properties to drop. Mutually exclusive with `properties_include`. Defaults to none.
- _`polygon_min_area`: f32 (optional)_ - Area in square tile-pixels below which a polygon is dropped. Defaults to `4`.
- _`polygon_simplify`: f32 (optional)_ - Douglas-Peucker tolerance for polygons, in tile-pixels. Defaults to `4`.
- _`line_min_length`: f32 (optional)_ - Length in tile-pixels below which a line is dropped. Defaults to `4`.
- _`line_simplify`: f32 (optional)_ - Douglas-Peucker tolerance for lines, in tile-pixels. Defaults to `4`.
- _`point_reduction`: PointReductionStrategy (optional)_ - Values: `none`, `drop_rate`, `min_distance`. How to thin out points too close to distinguish. Defaults to `min_distance`.
- _`point_reduction_value`: f32 (optional)_ - Distance in tile-pixels for `min_distance`, keep-fraction for `drop_rate`. Defaults to `16`/`0.5`.
- _`compression`: TileCompression (optional)_ - Values: `none`, `gzip`, `brotli`, `zstd`. Compression applied before the tiles leave. Defaults to `gzip`.
- _`max_tile_bytes`: u32 | none (optional)_ - Size in bytes above which a tile counts as broken. Defaults to `1048576`.
- _`ignore_id`: bool (optional)_ - Whether to drop each feature's `id` before encoding. Defaults to `false`.

---

## from_grid

Generates vector tiles holding the cells of a projected square grid.

Gridded statistics are published as a table keyed on a cell id, without the geometry that id refers to. This generates that geometry, so the table can be joined onto it with `vector_update_properties`.

Without GDAL, `epsg` accepts `3035` (ETRS89-LAEA, what European gridded statistics use), `3857` (web mercator) and `4326` (WGS84 lon/lat). A build with the `gdal` feature accepts any code, at roughly ten times the cost per coordinate — and released binaries ship without GDAL, so a `.vpl` naming another code will not run on one.

`bbox` is required: a grid of squares has no extent beyond the one it is given, and in a projected CRS an extent outside the projection's own area is not so much wrong as meaningless.

Within that bbox the grid covers every zoom from the level its cells become legible at up to level 30. Nothing is generated until a tile is asked for, so bound the work where it is done: `versatiles convert --max-zoom`, or a `filter` operation.

`id_template` builds the cell id from literal text and placeholders: `{x}` and `{y}` for the corner, `{epsg}` and `{size}` for the grid's own arguments, each taking an optional divisor (`{x/1000}`), a sign style and a zero-padded width (`{x/100:h04}`). The sign style is `-` for a minus sign when negative (the default), `+` for one either way, `h` for a hemisphere letter — N/S for `y`, E/W for `x` — before the digits, and `H` for one after them.

| Published as                                             | `id_template`                       |
| -------------------------------------------------------- | ----------------------------------- |
| `CRS3035RES1000mN2691000E4341000` (INSPIRE, the default) | `CRS{epsg}RES{size}m{y:h}{x:h}`     |
| `1kmN2689E4337` (GEOSTAT short form)                     | `{size/1000}km{y/1000:h}{x/1000:h}` |
| `E0643N4567` (CBS Netherlands)                           | `{x/100:h04}{y/100:h04}`            |
| `250mN674400E31725` (Statistics Finland)                 | `{size}m{y/10:h}{x/10:h}`           |

Cell size is fixed by `size` and does not change with zoom — that is what keeps an id stable enough to join against — so low zoom levels are unusable, and the pyramid starts at the level where a tile holds at most `max_cells_per_tile` cells.

A cell reaching into several tiles is drawn in each of them, clipped to the tile it is in. The same id therefore recurs across tiles, which is what a join expects — but the geometry carrying it in any one tile is only the part inside that tile, so it is not something to measure areas from.

### Parameters

- **`epsg`: u32 (required)** - EPSG code of the grid's coordinate reference system.
- **`size`: f64 (required)** - Edge length of a cell, in the CRS's units — meters, or degrees for `epsg=4326`.
- **`bbox`: [f64,f64,f64,f64] (required)** - Area to cover, as `[west, south, east, north]` in WGS84 degrees.
- _`offset`: [f64,f64] (optional)_ - Lower-left corner of cell `(0, 0)`, in CRS units. Defaults to `[0,0]`.
- _`max_cells_per_tile`: u32 (optional)_ - Roughly how many cells one tile may hold. Defaults to `1024`.
- _`id_template`: String (optional)_ - Cell id format. Defaults to `CRS{epsg}RES{size}m{y:h}{x:h}`.
- _`id_field`: String (optional)_ - Property holding the cell id. Defaults to `id`.
- _`x_field`: String (optional)_ - Property holding the corner's easting. Defaults to `x`.
- _`y_field`: String (optional)_ - Property holding the corner's northing. Defaults to `y`.
- _`densify_tolerance`: f64 (optional)_ - How far a cell edge may stray from its true curve, in tile pixels. Defaults to `0.5`.
- _`layer_name`: String (optional)_ - Name of the layer to write into. Defaults to `grid`.

---

## from_h3

Generates vector tiles holding H3 hexagons.

The hexagonal counterpart to `from_grid`: data published as a table keyed on an H3 index gets the geometry that index refers to, ready to be joined onto with `vector_update_properties`.

Resolution `0` gives cells of about 4,250,000 km² and `15` about 0.9 m²; `resolution=8` lands near 0.7 km². The full table of cell areas and edge lengths is at <https://h3geo.org/docs/core-library/restable/>.

The source covers the whole planet, from the zoom its cells become legible at up to level 30. Nothing is generated until a tile is asked for, so bound the work where it is done instead: `versatiles convert --bbox --max-zoom`, or a `filter` operation.

Cell size is fixed by the resolution and does not change with zoom — that is what keeps an id stable enough to join against — so low zoom levels are unusable, and the pyramid starts at the level where a tile holds at most `max_cells_per_tile` cells. Cells are measured at the equator, where mercator stretches them least, so the level it starts at holds everywhere.

A cell reaching into several tiles is drawn in each of them, clipped to the tile it is in. The same id therefore recurs across tiles, which is what a join expects — but the geometry carrying it in any one tile is only the part inside that tile, so it is not something to measure areas from.

### Parameters

- **`resolution`: u8 (required)** - H3 resolution, `0` (coarsest) to `15` (finest).
- _`max_cells_per_tile`: u32 (optional)_ - Roughly how many cells one tile may hold. Defaults to `1024`.
- _`layer_name`: String (optional)_ - Name of the layer to write into. Defaults to `grid`.
- _`id_field`: String (optional)_ - Property holding the H3 index. Defaults to `h3`.

---

## from_merged_vector

Merges several vector tile sources into one, keeping every feature.

Each output tile carries the features and properties of all the sources at that coordinate, so layers with the same name end up side by side rather than one replacing the other.

### Sources

The sources to merge, all of which must provide vector tiles.

---

## from_stacked

Overlays several tile sources, taking each tile from the first source that has it.

### Sources

The sources to stack, in priority order, all with the same format.

---

## from_stacked_raster

Blends several raster tile sources into one by alpha-compositing them.

Unlike `from_stacked`, which picks one source's tile whole, this composites them pixel by pixel, so a translucent source lets the ones beneath it show through.

With `auto_overscale=true`, a request that no source covers natively returns an empty stream rather than a blank tile. Put a `raster_overscale` _after_ `from_stacked_raster` to fill those in — upscaling one blended tile is cheaper than upscaling each source separately.

### Sources

The sources to blend, with the first on top. All must provide raster tiles at the same resolution.

### Parameters

- _`format`: RasterTileFormat (optional)_ - Values: `avif`, `jpg`, `png`, `webp`. Format to encode the blended tiles in. Defaults to the first source's.
- _`auto_overscale`: bool (optional)_ - Whether to wrap each source in `raster_overscale`. Defaults to `false`.

---

## from_tile

Reads one tile file and returns it for every requested coordinate.

### Parameters

- **`filename`: String (required)** - Path to the tile file; its format comes from the extension.

---

## from_tilejson

Reads tiles from a remote tile server described by a TileJSON endpoint.

The TileJSON document is fetched once when the pipeline is built, and each tile is then requested individually through the URL template in its `tiles` array.

### Parameters

- **`url`: String (required)** - URL of the TileJSON endpoint.
- _`max_retries`: u16 (optional)_ - How often to retry a failed tile request. Defaults to `3`.
- _`max_concurrent_requests`: u16 (optional)_ - How many tile requests may be in flight. Defaults to the I/O concurrency limit.

---

# TRANSFORM operations

---

## dem_overview

Generates lower-zoom DEM overview tiles by averaging 24-bit elevation values.

`raster_overview` averages the R, G and B channels independently, which is meaningless for a DEM: the channels are one number split across three bytes, so averaging them separately mixes the high byte of one pixel with the low byte of another. This operation decodes each pixel to its 24-bit raw elevation, averages that, and re-encodes.

### Parameters

- _`level`: u8 (optional)_ - Zoom level to build the overview from. Defaults to the source's highest.
- _`encoding`: DemEncoding (optional)_ - Values: `mapbox`, `terrarium`. DEM encoding of the source. Defaults to the encoding its tile schema implies.

---

## dem_quantize

Quantizes DEM raster tiles by rounding elevations to a per-tile power-of-two step.

Discarding low bits that carry no usable signal makes the tiles compress much better. The step is derived per tile from two physical criteria — `elevation_error` relative to the pixel's ground size, and `slope_error` — and the stricter of the two wins.

Values are rounded to the nearest multiple of the step rather than truncated, which halves the worst-case elevation error and removes the downward bias at no cost in size. Single-pass: no scan of the data first.

### Parameters

- _`elevation_error`: f64 (optional)_ - Allowed elevation error as a fraction of the pixel's ground size. Defaults to `0.1`.
- _`slope_error`: f64 (optional)_ - Largest slope change in degrees that quantization may introduce. Defaults to `1.0`.
- _`encoding`: DemEncoding (optional)_ - Values: `mapbox`, `terrarium`. DEM encoding of the source. Defaults to the encoding its tile schema implies.

---

## dem_tile_resize

Converts DEM tiles between 256 and 512 pixels by splitting or merging them.

The counterpart to `raster_tile_resize` for elevation data: downscaling averages the decoded 24-bit elevation rather than the R, G and B channels separately, which for a DEM would mix the high byte of one pixel with the low byte of another.

### Parameters

- **`tile_size`: u32 (required)** - Target tile size in pixels, `256` or `512`, and it must differ from the source's.
- _`encoding`: DemEncoding (optional)_ - Values: `mapbox`, `terrarium`. DEM encoding of the source. Defaults to the encoding its tile schema implies.

---

## filter

Filters tiles by bounding box, zoom range, or the coordinates present in another container.

Every parameter narrows the tile set, except `bbox_border`, which widens it. A `bbox_border=2` keeps a ring of tiles the bbox alone would have dropped; those tiles lie outside the crop, so the advertised bounds are extended to cover them and a client actually requests them.

That ring matters wherever a cropped tileset is rendered rather than just stored: without it, labels and geometry near the edge have no neighbouring tiles to be laid out against.

`filename` takes the same path and URL syntax as `from_container`. Opening it to build the allow-list costs I/O when the pipeline is built, unlike every other parameter here.

### Parameters

- _`bbox`: [f64,f64,f64,f64] (optional)_ - Area to keep, in WGS84 degrees. Defaults to the source's own bounds.
- _`bbox_border`: u32 (optional)_ - Ring of extra tiles kept around `bbox`, per zoom level. Requires `bbox`. Defaults to `0`.
- _`level_min`: u8 (optional)_ - Lowest zoom level to keep. Defaults to the source's lowest.
- _`level_max`: u8 (optional)_ - Highest zoom level to keep. Defaults to the source's highest.
- _`filename`: String (optional)_ - Tile container whose coordinates act as an allow-list. Defaults to no allow-list.

---

## meta_update

Overwrites fields of the source's TileJSON metadata.

Three ways to supply the new values, applied in that order: a whole document via `tilejson` or `tilejson_file` replaces the source's, `tilejson_update` or `tilejson_update_file` merges onto it, and the individual parameters below override whatever the first two produced.

Each of those pairs is mutually exclusive: `tilejson` with `tilejson_file`, `tilejson_update` with `tilejson_update_file`, `vector_layers` with `vector_layers_file`. The `_file` form exists to avoid quoting JSON inline.

A merge overwrites scalar fields and `vector_layers`, and widens `bounds` and the zoom range to the union.

The fields and their meaning follow the TileJSON 3.0.0 specification: <https://github.com/mapbox/tilejson-spec/tree/master/3.0.0>

### Parameters

- _`attribution`: String (optional)_ - Attribution text. Defaults to the source's.
- _`bounds`: [f64,f64,f64,f64] (optional)_ - Area covered, as `[west, south, east, north]` in WGS84 degrees. Defaults to the source's.
- _`center`: [f64,f64,f64] (optional)_ - Where a client should open the map, as `[lon, lat, zoom]`. Defaults to the source's.
- _`description`: String (optional)_ - Description text. Defaults to the source's.
- _`fillzoom`: u8 (optional)_ - Zoom level from which clients should fill from the parent tile. Defaults to the source's.
- _`legend`: String (optional)_ - Legend text. Defaults to the source's.
- _`name`: String (optional)_ - Name of the tileset. Defaults to the source's.
- _`schema`: TileSchema (optional)_ - Values: `rgb`, `rgba`, `dem/mapbox`, `dem/terrarium`, `dem/versatiles`, `openmaptiles`, `shortbread@1.0`, `other`. What the tiles contain. Defaults to the source's.
- _`tilejson`: String (optional)_ - Complete TileJSON document, as a JSON string. Defaults to the source's metadata.
- _`tilejson_file`: String (optional)_ - Path to a file holding a complete TileJSON document. Defaults to the source's metadata.
- _`tilejson_update`: String (optional)_ - Partial TileJSON document to merge on, as a JSON string. Defaults to merging nothing.
- _`tilejson_update_file`: String (optional)_ - Path to a file holding a partial TileJSON document. Defaults to merging nothing.
- _`vector_layers`: String (optional)_ - The `vector_layers` array as a JSON string. Defaults to the source's.
- _`vector_layers_file`: String (optional)_ - Path to a file holding the `vector_layers` array as JSON. Defaults to the source's.

---

## raster_flatten

Composites translucent raster tiles onto an opaque background colour.

### Parameters

- _`color`: [u8,u8,u8] (optional)_ - Background colour, as `[r, g, b]`. Defaults to white.

---

## raster_format

Re-encodes raster tiles into another image format, quality or effort setting.

`quality` and `quality_translucent` take a zoom-dependent list as well as a single number. In `quality="70,14:50,15:20"` the first value is the default and each `zoom:value` pair applies from that zoom level upwards — so zoom 0 to 13 use 70, zoom 14 uses 50, and zoom 15 and above use 20. Tiles that are already in the target format and need no quality change are passed through without re-encoding. `quality` is ignored for PNG, which is always lossless.

`quality_translucent` is typically `100`: lossy encoders handle an alpha channel badly. Setting it makes every tile be checked for opacity.

### Parameters

- _`format`: RasterTileFormat (optional)_ - Values: `avif`, `jpg`, `png`, `webp`. Format to encode the tiles into. Defaults to the source's.
- _`quality`: String (optional)_ - Encoder quality, `0` (worst) to `100` (lossless). Defaults to the encoder's own.
- _`quality_translucent`: String (optional)_ - Encoder quality for tiles with translucent pixels. Defaults to using `quality` throughout.
- _`effort`: u8 (optional)_ - Encoder effort, `0` (fastest) to `100` (smallest). Defaults to the encoder's own.

---

## raster_levels

Adjusts the brightness, contrast and gamma of raster tiles.

### Parameters

- _`brightness`: f32 (optional)_ - Offset added to every channel, `-255` to `255`. Defaults to `0.0`.
- _`contrast`: f32 (optional)_ - Factor applied around mid-grey, above `0`. Defaults to `1.0`.
- _`gamma`: f32 (optional)_ - Gamma exponent, above `0`. Defaults to `1.0`.

---

## raster_mask

Makes raster pixels outside a GeoJSON polygon transparent.

The mask is not reprojected: coordinates outside the WGS84 range are refused, as is a `crs` member naming another projection. A mask written lat,lon is in range and cannot be detected — it masks the wrong part of the world.

### Parameters

- **`geojson`: String (required)** - Path to a GeoJSON file holding a Polygon or MultiPolygon, in EPSG:4326 lon/lat degrees.
- _`buffer`: f32 (optional)_ - Distance in meters by which to grow the mask, or shrink it when negative. Defaults to `0`.
- _`blur`: f32 (optional)_ - Width in meters of the soft transition at the mask edge. Defaults to `0`.
- _`blur_function`: BlurFunction (optional)_ - Values: `linear`, `cosine`. Falloff curve across the `blur` band. Defaults to `linear`.

---

## raster_overscale

Serves raster tiles above the source's native resolution by upscaling.

Tiles at `level_base` and below are passed through unchanged; above it, the covering tile from `level_base` is cropped to the requested area and scaled up. The result is blurry rather than detailed — the point is that a client can keep zooming instead of hitting a blank map.

### Parameters

- _`level_base`: u8 (optional)_ - Zoom level to upscale from. Defaults to the source's highest.
- _`level_max`: u8 (optional)_ - Highest zoom level to serve. Defaults to `30`.
- _`enable_climbing`: bool (optional)_ - Whether to climb to lower levels when the `level_base` tile is missing. Defaults to `false`.

---

## raster_overview

Generates the lower zoom levels of a raster pyramid by downscaling.

### Parameters

- _`level`: u8 (optional)_ - Zoom level to build the overview from. Defaults to the source's highest.

---

## raster_tile_resize

Converts raster tiles between 256 and 512 pixels by splitting or merging them.

Changing the tile size shifts the zoom levels with it, because the ground resolution of a pixel has to stay the same. `tile_size=256` splits each 512-pixel tile into four 256-pixel tiles one zoom level higher, except at level 0, which has no level below it to move to and is downscaled instead. `tile_size=512` merges four 256-pixel tiles into one 512-pixel tile one zoom level lower.

### Parameters

- **`tile_size`: u32 (required)** - Target tile size in pixels, `256` or `512`, and it must differ from the source's.

---

## remap_coords

Relabels tile coordinates, correcting a source that uses TMS row order or `z/y/x` paths.

The three flags are applied in a fixed order — `flip_x`, then `flip_y`, then `swap_xy` — and between them reach all eight symmetries of the square: four rotations and four reflections, which is every relabelling that maps the tile grid onto itself. Because that set is closed, chaining two of these operations is never necessary; some single combination of the three flags does the same thing.

The combinations most likely to be wanted:

| `flip_x` | `flip_y` | `swap_xy` | result                    |
| -------- | -------- | --------- | ------------------------- |
| false    | true     | false     | TMS ↔ XYZ row order       |
| false    | false    | true      | `z/y/x` ↔ `z/x/y` layouts |
| true     | true     | false     | rotate 180°               |
| true     | false    | true      | rotate 90°                |

Unlike a global `--flip-y` flag, this applies to one source, so a pipeline can combine sources that disagree about their conventions.

### Parameters

- _`flip_x`: bool (optional)_ - Whether to mirror horizontally, so `x` becomes `2^z - 1 - x`. Defaults to `false`.
- _`flip_y`: bool (optional)_ - Whether to mirror vertically, so `y` becomes `2^z - 1 - y`. Defaults to `false`.
- _`swap_xy`: bool (optional)_ - Whether to exchange the axes, so `(x, y)` becomes `(y, x)`. Defaults to `false`.

---

## vector_filter_features

Drops vector features in selected layers that do not satisfy a boolean expression.

Features in layers outside `layer` pass through untouched.

### Examples

```vpl
vector_filter_features layer=["place"] expr="name == 'Berlin'"
vector_filter_features layer=["poi"]   expr="population >= 1000"
vector_filter_features layer=["road"]  expr="highway in ['primary','secondary']"
vector_filter_features layer=["place"] expr="name.matches('^St\\.')"
vector_filter_features layer=["poi"]   expr="name != null && name != ''"
vector_filter_features layer=["addr"]  expr="props['addr:street'] == 'Hauptstr.'"
```

### Expression language

`expr` is a boolean [CEL (Common Expression Language)](https://github.com/google/cel-spec) expression, evaluated once per feature.

**Types** — bool (`true`, `false`), int / uint (`42`, `-7`, `1000u`), double (`3.14`, `-0.5`, `1e-6`), string (`'hello'` or `"hello"`), list (`[1, 2, 3]`, `['a', 'b']`), map (`m['key']` or `m.key`), and `null`.

**Operators** — equality `==` `!=`, ordering `<` `<=` `>` `>=`, logical `&&` `||` `!`, membership `x in [1, 2, 3]`, and `s.matches('pattern')` for a regex in RE2 syntax, matched anywhere in `s`.

### Accessing feature properties

Properties whose names are valid CEL identifiers — letters, digits and underscore — are exposed as top-level variables:

```vpl
vector_filter_features layer=["place"] expr="name == 'Berlin'"
```

For keys containing `:`, `-`, `.`, or anything else that is not an identifier, use the `props` map:

```vpl
vector_filter_features layer=["addr"] expr="props['addr:street'] == 'Hauptstr.'"
```

### Missing keys

A property a feature does not carry resolves to `null` for identifier-safe access, so compare against `null` to say explicitly whether such features are kept or dropped:

```vpl
vector_filter_features layer=["place"] expr="name != null && name != ''"
```

The `has()` macro asks the same question of an identifier-safe key, and `in` of any key:

```vpl
vector_filter_features layer=["place"] expr="has(props.name)"
vector_filter_features layer=["addr"]  expr="'addr:street' in props"
```

The [CEL language spec](https://github.com/google/cel-spec/blob/master/doc/langdef.md) has the full grammar, built-in functions and string methods.

### Parameters

- **`layer`: [String,...] (required)** - Layers the expression applies to, for example `layer=["poi","place"]`.
- **`expr`: String (required)** - Boolean CEL expression over the feature's properties.

---

## vector_filter_layers

Removes whole layers from vector tiles by name.

### Parameters

- **`filter`: [String,...] (required)** - Layer names to remove, for example `filter=["pois","ocean"]`.
- _`invert`: bool (optional)_ - Whether to keep the named layers instead of removing them. Defaults to `false`.

---

## vector_filter_properties

Removes feature properties from vector tiles by matching their names against a regex.

A property's name is matched with its layer as a prefix, in the form `layer_name/property_name`. That makes it possible to target one layer — `regex="^places/"` drops every property of the `places` layer — or to reach across all of them, as `regex="/name_.*$"` does for every property starting with `name_`.

### Parameters

- **`regex`: String (required)** - Regular expression matched against each property's prefixed name.
- _`invert`: bool (optional)_ - Whether to keep the matching properties instead of removing them. Defaults to `false`.

---

## vector_overzoom

Serves vector tiles above the source's highest zoom level by clipping and rescaling.

Tiles at `level_base` and below are passed through unchanged; above it, the covering tile from `level_base` is clipped to the requested sub-region and its coordinates rescaled. No detail is added — the geometry is the parent's — but a client can keep zooming instead of hitting a blank map.

`level_max` defaults to four levels above `level_base` because each extra level quadruples the tile count, and four is usually as far as the pyramid stays manageable.

### Parameters

- _`level_base`: u8 (optional)_ - Zoom level to overzoom from. Defaults to the source's highest.
- _`level_max`: u8 (optional)_ - Highest zoom level to serve, capped at `30`. Defaults to `level_base + 4`.
- _`enable_climbing`: bool (optional)_ - Whether to climb to lower levels when the `level_base` tile is missing. Defaults to `false`.
- _`buffer`: u32 (optional)_ - Clip buffer in tile-extent units, so edge-straddling features survive. Defaults to `80`.

---

## vector_repair

Repairs vector tiles so that they conform to MVT 2.1.

Always fixed: missing `extent` and `version` fields, duplicate layer names, inverted polygon winding, and degenerate rings.

Tiles the validator considers clean pass through unchanged — the original encoded blob is forwarded without re-encoding — so this operation is cheap on conformant input.

Without `drop_offenders`, a layer holding a feature whose geometry cannot be decoded keeps its original geometry bytes, while the structural fixes are still applied to it.

### Example

```vpl
from_container filename="bad.versatiles" | vector_repair
from_container filename="bad.versatiles" | vector_repair drop_offenders=true
```

### Parameters

- _`drop_offenders`: bool (optional)_ - Whether to remove features whose geometry cannot be decoded. Defaults to `false`.

---

## vector_update_properties

Joins tabular data onto vector features, matching on an id column.

Each row of a CSV or TSV file is matched to the features whose `id_field_tiles` property equals the row's `id_field_data` column, and the row's remaining columns become properties on those features. This is how a published statistics table is attached to the geometry it refers to — see `from_grid` and `from_h3` for generating that geometry.

### Parameters

- **`data_source_path`: String (required)** - Path to the CSV or TSV file, which must have a header row.
- **`layer_name`: String (required)** - Name of the layer whose features are updated.
- **`id_field_tiles`: String (required)** - Feature property holding the id to match on.
- **`id_field_data`: String (required)** - Column in the data file holding the id to match on.
- _`replace_properties`: bool (optional)_ - Whether to replace a feature's properties instead of merging. Defaults to `false`.
- _`remove_non_matching`: bool (optional)_ - Whether to drop features that have no matching row. Defaults to `false`.
- _`include_id`: bool (optional)_ - Whether to keep the id column among the written properties. Defaults to `false`.
- _`field_separator`: String (optional)_ - Character separating a row's fields. Defaults to `,` for `.csv` and a tab for `.tsv`.
- _`decimal_separator`: String (optional)_ - Decimal separator for parsing numbers, so `,` reads `1.234,56`. Defaults to `.`.
