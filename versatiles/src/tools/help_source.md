# Data Source Syntax

VersaTiles supports multiple ways to specify data sources for tile containers.

## Basic Usage

### File Paths

Local file paths are resolved relative to the current directory:

```text
tiles.versatiles
./data/world.mbtiles
/absolute/path/to/tiles.pmtiles
```

### URLs

HTTP and HTTPS URLs are supported (VersaTiles containers only for remote):

```text
https://example.org/tiles.versatiles
http://download.example.org/world.versatiles
```

## Name and Type Prefixes

You can override the auto-detected name and container type using bracket notation.

### Prefix Notation

Format: `[name,type]location`

```text
[osm,mbtiles]tiles.db        # Set name to "osm", type to "mbtiles"
[,mbtiles]tiles.db           # Set only type to "mbtiles"
[osm]tiles.mbtiles           # Set only name to "osm"
[osm,vpl]pipeline.txt        # Treat file as VPL pipeline with name "osm"
```

### Postfix Notation

Format: `location[name,type]`

```text
tiles.db[osm,mbtiles]        # Same as [osm,mbtiles]tiles.db
tiles.db[,mbtiles]           # Same as [,mbtiles]tiles.db
tiles.mbtiles[osm]           # Same as [osm]tiles.mbtiles
```

## Inline VPL Pipelines

Use `[,vpl]` prefix with parentheses to define a VPL pipeline directly:

```text
[,vpl](from_mbtiles tiles.mbtiles)
[osm,vpl](from_mbtiles tiles.mbtiles | filter_zoom 0-10)
```

The content in parentheses is treated as VPL (VersaTiles Pipeline Language).

## JSON Format

For programmatic use, data sources can be specified as JSON:

```json
{"location": "tiles.mbtiles"}
{"name": "osm", "type": "mbtiles", "location": "tiles.db"}
{"name": "inline", "type": "vpl", "content": "from_debug"}
```

JSON fields:

- `location`: Path or URL to the data source
- `name`: Optional name identifier (defaults to filename without extension)
- `type`: Optional container type (defaults to file extension)
- `content`: Inline content (alternative to `location`)

## Supported Container Types

- `versatiles` - VersaTiles format (*.versatiles)
- `mbtiles` - MBTiles SQLite format (*.mbtiles)
- `pmtiles` - PMTiles format (*.pmtiles)
- `tar` - Tar archive (*.tar)
- `vpl` - VersaTiles Pipeline Language (*.vpl)
- Directory containing tiles in `{z}/{x}/{y}.{ext}` structure

## Examples

```bash
# Basic file
versatiles convert tiles.mbtiles output.versatiles

# Remote VersaTiles container
versatiles probe https://download.versatiles.org/osm.versatiles

# Named tile source for serving
versatiles serve [osm]tiles.versatiles [satellite]imagery.mbtiles

# Inline VPL pipeline
versatiles convert "[,vpl](from_mbtiles in.mbtiles | filter_zoom 0-12)" out.versatiles

# Override container type
versatiles probe tiles.db[,mbtiles]
```
