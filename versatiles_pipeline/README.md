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
Reads a tile container, such as a VersaTiles file.
### Parameters:
* **`filename`: String (required)** - The filename of the tile container. This is relative to the path of the VPL file. For example: `filename="world.versatiles"`.

## from_debug
Produces debugging tiles, each showing their coordinates as text.
### Parameters:
* **`format`: String (required)** - tile format: "mvt", "jpg", "png" or "webp"

## from_overlayed
Overlays multiple tile sources, using the tile from the first source that provides it.
### Sources:
All tile sources must have the same format.

## from_vectortiles_merged
Merges multiple vector tile sources. Each layer will contain all features from the same layer of all sources.
### Sources:
All tile sources must provide vector tiles.

---
# TRANSFORM operations

## filter_bbox
Filter tiles by a geographic bounding box.
### Parameters:
* **`bbox`: [f64,f64,f64,f64] (required)** - Bounding box: [min long, min lat, max long, max lat].

## filter_zoom
Filter tiles by zoom level.
### Parameters:
* *`min`: u8 (optional)* - minimal zoom level
* *`max`: u8 (optional)* - maximal zoom level

## vectortiles_update_properties
Updates properties of vector tile features using data from an external source (e.g., CSV file). Matches features based on an ID field.
### Parameters:
* **`data_source_path`: String (required)** - Path to the data source file, e.g., `data_source_path="data.csv"`.
* **`layer_name`: String (required)** - Name of the vector layer to update.
* **`id_field_tiles`: String (required)** - ID field name in the vector layer.
* **`id_field_data`: String (required)** - ID field name in the data source.
* *`replace_properties`: Boolean (optional, default: false)* - If set, old properties will be deleted before new ones are added.
* *`remove_non_matching`: Boolean (optional, default: false)* - If set, removes all features (in the layer) that do not match.
* *`include_id`: Boolean (optional, default: false)* - If set, includes the ID field in the updated properties.

