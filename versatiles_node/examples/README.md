# VersaTiles Node.js Examples

This directory contains practical examples demonstrating the main features of the VersaTiles Node.js bindings.

## Prerequisites

Before running the examples, make sure you have:
1. Installed the package: `npm install` in the `versatiles_node` directory
2. Built the native bindings: `npm run build`
3. Test data available at `../testdata/berlin.mbtiles`

## Examples

### 1. convert.js - Tile Format Conversion

Demonstrates various ways to convert tiles between formats.

```bash
node examples/convert.js
```

**Features shown:**
- Simple format conversion (MBTiles → VersaTiles)
- Filtering by zoom level
- Filtering by bounding box
- Adding border tiles around a bounding box
- Applying compression (gzip, brotli)
- Coordinate transformations (flip_y, swap_xy)

**Output:** Creates several `.versatiles` files in the `examples/` directory

### 2. probe.js - Container Inspection

Shows how to inspect tile containers and retrieve metadata.

```bash
node examples/probe.js
```

**Features shown:**
- Quick probe using `probeTiles()` function
- Detailed inspection using `ContainerReader`
- Accessing TileJSON metadata
- Reading container parameters (format, compression, zoom levels)
- Displaying geographic coverage and center point
- Comparing multiple containers

**Output:** Prints detailed information about the container to the console

### 3. serve.js - HTTP Tile Server

Demonstrates how to serve tiles via HTTP server.

```bash
# Run basic example (default)
node examples/serve.js

# Run specific example
node examples/serve.js 1  # Basic server
node examples/serve.js 2  # Multiple sources
node examples/serve.js 3  # Static files
node examples/serve.js 4  # Dynamic sources
```

**Features shown:**
- Starting a basic HTTP server
- Adding tile sources
- Adding static file sources
- Dynamic source management (add sources while running)
- Graceful shutdown

**URLs available:**
- Tiles: `http://127.0.0.1:8080/tiles/berlin/{z}/{x}/{y}`
- TileJSON: `http://127.0.0.1:8080/tiles/berlin/meta.json`
- Status: `http://127.0.0.1:8080/status`

**Stop:** Press `Ctrl+C`

### 4. read-tiles.js - Tile Reading and Coordinates

Shows how to read individual tiles and work with coordinates.

```bash
node examples/read-tiles.js
```

**Features shown:**
- Reading a single tile
- Reading multiple tiles
- Converting geographic coordinates to tile coordinates
- Converting tile coordinates to geographic coordinates
- Getting tile bounding boxes
- Reading all tiles in a geographic area
- Checking tile availability
- Saving tiles to files

**Output:**
- Prints tile information to the console
- Saves a sample tile as `tile-10-550-335.png`

## Example Data

All examples use the test data file `../testdata/berlin.mbtiles`. This file should contain map tiles for the Berlin area.

If you don't have this test data, you can:
1. Use your own tile container files
2. Modify the examples to point to your data
3. Download sample data from [VersaTiles](https://versatiles.org)

## Common Patterns

### Opening a Container

```javascript
const { ContainerReader } = require('@versatiles/versatiles');

const reader = await ContainerReader.open('path/to/tiles.mbtiles');
```

### Getting Tile Metadata

```javascript
const tileJSON = JSON.parse(await reader.tileJSON);
const params = await reader.parameters;

console.log('Format:', params.tileFormat);
console.log('Zoom:', params.minZoom, '-', params.maxZoom);
```

### Converting Coordinates

```javascript
const { TileCoord } = require('@versatiles/versatiles');

// Geographic → Tile
const coord = TileCoord.fromGeo(13.405, 52.520, 10);
console.log(`Tile: ${coord.z}/${coord.x}/${coord.y}`);

// Tile → Geographic
const tile = new TileCoord(10, 550, 335);
const [lon, lat] = tile.toGeo();
console.log(`Location: ${lon}°, ${lat}°`);
```

### Error Handling

```javascript
try {
  await convertTiles('input.mbtiles', 'output.versatiles');
} catch (err) {
  console.error('Conversion failed:', err.message);
  // Handle error appropriately
}
```

## Tips

1. **Performance**: The bindings are implemented in Rust for maximum performance. Operations on large tile sets are fast and memory-efficient.

2. **Async/Await**: All I/O operations are async. Always use `await` or `.then()` when calling these methods.

3. **Resource Cleanup**: The server automatically cleans up resources when stopped. For containers, the reader is automatically closed when garbage collected.

4. **Error Messages**: Error messages from the Rust layer are passed through with full context. They're helpful for debugging.

5. **File Formats**: The bindings support:
   - `.versatiles` - Native VersaTiles format
   - `.mbtiles` - MBTiles (SQLite-based)
   - `.pmtiles` - PMTiles (cloud-optimized)
   - `.tar` - TAR archives
   - Directories - File system based

## Troubleshooting

### "Cannot find module '../index.js'"

Make sure you've built the native bindings:
```bash
cd versatiles_node
npm install
npm run build
```

### "ENOENT: no such file or directory"

Check that the test data file exists:
```bash
ls ../testdata/berlin.mbtiles
```

### Server port already in use

Change the port in the server examples:
```javascript
const server = new TileServer({ port: 8081 }); // Use a different port
```

## Next Steps

After exploring these examples:
1. Check out the [API Documentation](../README.md) for complete reference
2. Read the [VersaTiles Documentation](https://docs.versatiles.org/) for more information
3. Try integrating VersaTiles into your own projects

## Contributing

Found a bug or have a suggestion? Please open an issue on [GitHub](https://github.com/versatiles-org/versatiles-rs/issues).
