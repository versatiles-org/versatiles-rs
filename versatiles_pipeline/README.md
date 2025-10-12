# VersaTiles Pipeline

VersaTiles Pipeline is a robust toolkit designed for efficiently generating and processing large volumes of tiles. It leverages multithreading to stream, process, and transform tiles from one or more sources in parallel, either storing them in a new tile container or delivering them in real-time through a server:

```bash
# save the processed tiles in a container:
versatiles convert pipeline.vpl result.versatiles

# serve the tiles directy via the server:
versatiles serve pipeline.vpl
```

## Defining a pipeline

To define a pipeline, create a .vpl file and descibe the pipeline using the VersaTiles Pipeline Language (VPL). Pipelines always begin with a read operation (name starts with "from_"), optionally followed by one or more transform operations, separated by the pipe symbol (`|`).

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
from_overlayed [
   from_container filename="world.versatiles",
   from_container filename="europe.versatiles" | filter_zoom min=5,
   from_container filename="germany.versatiles"
]
```
---
# READ operations

## from_container
Reads a tile container, such as a `*.versatiles`, `*.mbtiles`, `*.pmtiles` or `*.tar` file.
### Parameters:
- **`filename`: String (required)** - The filename of the tile container. This is relative to the path of the VPL file. For example: `filename="world.versatiles"`.

## from_debug
Generates debug tiles that display their coordinates as text.
### Parameters:
- *`format`: String (optional)* - Target tile format: one of `"mvt"` (default), `"avif"`, `"jpg"`, `"png"` or `"webp"`

## from_merged_vector
Merges multiple vector tile sources.
Each resulting tile will contain all the features and properties from all the sources.
### Sources:
All tile sources must provide vector tiles.

## from_stacked
Overlays multiple tile sources, using the tile from the first source that provides it.
### Sources:
All tile sources must have the same format.

## from_stacked_raster
Overlays multiple raster tile sources on top of each other.
### Sources:
All tile sources must provide raster tiles in the same resolution. The first source overlays the others.
### Parameters:
- *`format`: TileFormat (optional)* - The tile format to use for the output tiles. Default: format of the first source.

---
# TRANSFORM operations

## filter
Filter tiles by bounding box and/or zoom levels.
### Parameters:
- *`bbox`: [f64,f64,f64,f64] (optional)* - Bounding box: [min long, min lat, max long, max lat].
- *`level_min`: u8 (optional)* - minimal zoom level
- *`level_max`: u8 (optional)* - maximal zoom level

## meta_update
Update metadata, see also https://github.com/mapbox/tilejson-spec/tree/master/3.0.0
### Parameters:
- *`attribution`: String (optional)* - Attribution text.
- *`description`: String (optional)* - Description text.
- *`fillzoom`: u8 (optional)* - Fill zoom level.
- *`name`: String (optional)* - Name text.
- *`schema`: String (optional)* - Schema text.

## raster_flatten
Flattens (translucent) raster tiles onto a background
### Parameters:
- *`color`: [u8,u8,u8] (optional)* - background color to use for the flattened tiles, in RGB format. Defaults to white.

## raster_format
Filter tiles by bounding box and/or zoom levels.
### Parameters:
- *`format`: String (optional)* - The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP. If not specified, the source format will be used.
- *`quality`: String (optional)* - Quality level for the tile compression (only AVIF, JPG or WEBP), between 0 (worst) and 100 (lossless). To allow different quality levels for different zoom levels, this can also be a comma-separated list like this: "80,70,14:50,15:20", where the first value is the default quality, and the other values specify the quality for the specified zoom level (and higher).
- *`speed`: u8 (optional)* - Compression speed (only AVIF), between 0 (slowest) and 100 (fastest).

## raster_levels
Adjust brightness, contrast and gamma of raster tiles.
### Parameters:
- *`brightness`: f32 (optional)* - Brightness adjustment. Defaults to 0.0 (no change).
- *`contrast`: f32 (optional)* - Contrast adjustment. Defaults to 1.0 (no change).
- *`gamma`: f32 (optional)* - Gamma adjustment. Defaults to 1.0 (no change).

## raster_overscale
Filter tiles by bounding box and/or zoom levels.
### Parameters:
- *`level_base`: u8 (optional)* - use this zoom level to build the overscale. Defaults to the maximum zoom level of the source.
- *`level_max`: u8 (optional)* - use this as maximum zoom level. Defaults to 30.
- *`tile_size`: u32 (optional)* - Size of the tiles in pixels. Defaults to 512.

## raster_overview
Filter tiles by bounding box and/or zoom levels.
### Parameters:
- *`level`: u8 (optional)* - use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
- *`tile_size`: u32 (optional)* - Size of the tiles in pixels. Defaults to 512.

## vector_filter_layers
Filters vector tile layers based on a comma-separated list of layer names.
### Parameters:
- **`filter`: String (required)** - Comma‑separated list of layer names that should be removed from the tiles, e.g.: filter="pois,ocean".
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only layers matching the filter).

## vector_filter_properties
Filters properties based on a regular expressions.
### Parameters:
- **`regex`: String (required)** - A regular expression pattern that should match property names to be removed from all features. The property names contain the layer name as a prefix, e.g., `layer_name/property_name`, so an expression like `^layer_name/` will match all properties of that layer or `/name_.*$/` will match all properties starting with `name_` in all layers.
- *`invert`: bool (optional)* - If set, inverts the filter logic (i.e., keeps only properties matching the filter).

## vector_update_properties
Updates properties of vector tile features using data from an external source (e.g., CSV file). Matches features based on an ID field.
### Parameters:
- **`data_source_path`: String (required)** - Path to the data source file, e.g., `data_source_path="data.csv"`.
- **`layer_name`: String (required)** - Name of the vector layer to update.
- **`id_field_tiles`: String (required)** - ID field name in the vector layer.
- **`id_field_data`: String (required)** - ID field name in the data source.
- *`replace_properties`: bool (optional)* - If set, old properties will be deleted before new ones are added.
- *`remove_non_matching`: bool (optional)* - If set, removes all features (in the layer) that do not match.
- *`include_id`: bool (optional)* - If set, includes the ID field in the updated properties.

