# VersaTiles Node.js Examples

This directory contains practical examples demonstrating the main features of the VersaTiles Node.js bindings.

## Prerequisites

Before running the examples, make sure you have:

1. Installed the package: `npm install` in the `versatiles_node` directory
2. Built the native bindings: `npm run build:debug` (or `npm run build` for release)
3. Test data available at `../testdata/berlin.mbtiles` and `../testdata/berlin.pmtiles`

## Examples

All examples are written in TypeScript and use ESM imports. Run them with:

```bash
npx tsx examples/<filename>.ts
```

### 1. convert.ts - Tile Format Conversion

Demonstrates various ways to convert tiles between formats.

```bash
npx tsx examples/convert.ts
```

**Features shown:**

- Simple format conversion (MBTiles → VersaTiles)
- Filtering by zoom level
- Filtering by bounding box
- Adding border tiles around a bounding box
- Applying compression (gzip, brotli)
- Coordinate transformations (flipY, swapXy)

**Output:** Creates several `.versatiles` files in `/tmp/`

### 2. convert-with-progress.ts - Conversion with Progress Monitoring

Shows how to monitor conversion progress with callbacks.

```bash
npx tsx examples/convert-with-progress.ts
```

**Features shown:**

- Using progress callbacks to monitor conversion
- Real-time progress updates with percentage, speed, and ETA
- Message callbacks for warnings and errors

**Output:** Displays live progress bar during conversion

### 3. probe.ts - Container Inspection

Shows how to inspect tile containers and retrieve metadata.

```bash
npx tsx examples/probe.ts
```

**Features shown:**

- Opening tile sources with `TileSource.open()`
- Accessing source type information
- Reading metadata (format, compression, zoom levels)
- Accessing TileJSON metadata

**Output:** Prints detailed information about the container to the console

### 4. serve.ts - HTTP Tile Server

Demonstrates how to serve tiles via HTTP server.

```bash
npx tsx examples/serve.ts
```

**Features shown:**

- Starting a basic HTTP server
- Adding tile sources from file paths
- Dynamic source management (add/remove sources while running)
- Multiple server configurations
- Graceful shutdown

**URLs available:**

- Tiles: `http://127.0.0.1:8080/tiles/berlin/{z}/{x}/{y}`
- TileJSON: `http://127.0.0.1:8080/tiles/berlin/meta.json`
- Status: `http://127.0.0.1:8080/status`

**Note:** The example runs multiple server configurations sequentially, each for 1 second.

### 5. read-tiles.ts - Tile Reading and Coordinates

Shows how to read individual tiles and work with coordinates.

```bash
npx tsx examples/read-tiles.ts
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
- Saves a sample tile as `/tmp/tile-10-550-335.png`

## Example Data

All examples use test data files from `../testdata/`:

- `berlin.mbtiles` - MBTiles format
- `berlin.pmtiles` - PMTiles format

These files contain map tiles for the Berlin area.

If you don't have this test data, you can:

1. Use your own tile container files
2. Modify the examples to point to your data
3. Download sample data from [VersaTiles](https://versatiles.org)

## Common Patterns

### Opening a Container

```typescript
import { TileSource } from '@versatiles/versatiles-rs';

const source = await TileSource.open('path/to/tiles.mbtiles');
```

### Getting Tile Metadata

```typescript
import { TileSource } from '@versatiles/versatiles-rs';

const source = await TileSource.open('tiles.mbtiles');
const metadata = source.metadata();
const tileJSON = source.tileJson();

console.log('Format:', metadata.tileFormat);
console.log('Zoom:', metadata.minZoom, '-', metadata.maxZoom);
console.log('Bounds:', tileJSON.bounds);
```

### Converting Coordinates

```typescript
import { TileCoord } from '@versatiles/versatiles-rs';

// Geographic → Tile
const coord = TileCoord.fromGeo(13.405, 52.52, 10);
console.log(`Tile: ${coord.z}/${coord.x}/${coord.y}`);

// Tile → Geographic
const tile = new TileCoord(10, 550, 335);
const [lon, lat] = tile.toGeo();
console.log(`Location: ${lon}°, ${lat}°`);
```

### Converting Tiles

```typescript
import { convert } from '@versatiles/versatiles-rs';

await convert('input.mbtiles', 'output.versatiles', {
  minZoom: 5,
  maxZoom: 12,
  compress: 'gzip',
});
```

### Error Handling

```typescript
import { convert } from '@versatiles/versatiles-rs';

try {
  await convert('input.mbtiles', 'output.versatiles');
} catch (err) {
  console.error('Conversion failed:', err.message);
  // Handle error appropriately
}
```

### Using with CommonJS

If you need to use CommonJS instead of ESM:

```javascript
const { convert, TileSource, TileServer, TileCoord } = require('@versatiles/versatiles-rs');

// Use the same API as shown above
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

### "Cannot find module '@versatiles/versatiles-rs'"

Make sure you've built the native bindings:

```bash
cd versatiles_node
npm install
npm run build:debug
```

### "ENOENT: no such file or directory"

Check that the test data files exist:

```bash
ls ../testdata/berlin.mbtiles
ls ../testdata/berlin.pmtiles
```

### "Cannot find native binding"

Rebuild the native bindings:

```bash
npm run build:debug
```

### Server port already in use

The server examples run on port 8080. If it's already in use, you can modify the example files to use a different port.

### TypeScript/ESM issues

All examples use ESM imports and TypeScript. They're designed to be run with `tsx`:

```bash
npx tsx examples/probe.ts
```

If you prefer CommonJS, you can convert the imports to require() statements.

## Next Steps

After exploring these examples:

1. Check out the [API Documentation](../README.md) for complete reference
2. Read the [VersaTiles Documentation](https://docs.versatiles.org/) for more information
3. Try integrating VersaTiles into your own projects

## Contributing

Found a bug or have a suggestion? Please open an issue on [GitHub](https://github.com/versatiles-org/versatiles-rs/issues).
