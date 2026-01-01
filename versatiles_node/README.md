# @versatiles/versatiles-rs

Node.js bindings for [VersaTiles](https://github.com/versatiles-org/versatiles-rs) - convert, serve, and process map tiles in various formats.

## Features

- 🚀 **Fast & Native** - Powered by Rust with zero-copy operations
- 🔄 **Format Conversion** - Convert between MBTiles, PMTiles, VersaTiles, TAR, and directories
- 🗺️ **Tile Server** - Built-in HTTP tile server with dynamic source management
- 📊 **Metadata Access** - Read TileJSON and inspect container details
- 🌍 **Coordinate Utils** - Convert between tile and geographic coordinates
- ⚡ **Async API** - Non-blocking operations with Promise-based interface
- 📦 **Dual Format** - Supports both ESM and CommonJS

## Installation

```bash
npm install @versatiles/versatiles-rs
# or
yarn add @versatiles/versatiles-rs
```

Pre-built binaries are available for:

- macOS (arm64, x64)
- Linux (x64, arm64, musl)
- Windows (x64, arm64)

## Quick Start

### Convert Tiles

```javascript
import { convert } from '@versatiles/versatiles-rs';

await convert('input.mbtiles', 'output.versatiles', {
  minZoom: 0,
  maxZoom: 14,
  bbox: [-180, -85, 180, 85],
  compress: 'gzip',
});
```

### Serve Tiles

```javascript
import { TileServer } from '@versatiles/versatiles-rs';

const server = new TileServer({ port: 8080 });
await server.addTileSourceFromPath('osm', 'tiles.mbtiles');
await server.start();

console.log(`Server running at http://localhost:${await server.port}`);
```

### Read Tiles

```javascript
import { TileSource } from '@versatiles/versatiles-rs';

const source = await TileSource.open('tiles.mbtiles');

// Get a single tile
const tile = await source.getTile(5, 16, 10);
if (tile) {
  console.log('Tile size:', tile.length, 'bytes');
}

// Get metadata
const metadata = source.metadata();
console.log('Format:', metadata.tileFormat);
console.log('Zoom levels:', metadata.minZoom, '-', metadata.maxZoom);

// Get TileJSON
const tileJSON = source.tileJson();
console.log('Bounds:', tileJSON.bounds);
```

### Probe Container

```javascript
import { TileSource } from '@versatiles/versatiles-rs';

const source = await TileSource.open('tiles.mbtiles');
const sourceType = source.sourceType();
const metadata = source.metadata();

console.log('Type:', sourceType.kind);
console.log('Format:', metadata.tileFormat);
console.log('Compression:', metadata.tileCompression);
```

### Coordinate Conversion

```javascript
import { TileCoord } from '@versatiles/versatiles-rs';

// Geographic to tile coordinates
const coord = TileCoord.fromGeo(13.4, 52.5, 10);
console.log(`Tile: z=${coord.z}, x=${coord.x}, y=${coord.y}`);

// Tile to geographic coordinates
const tile = new TileCoord(10, 550, 335);
const [lon, lat] = tile.toGeo();
console.log(`Location: ${lon}, ${lat}`);

// Get bounding box
const bbox = tile.toGeoBbox();
console.log('BBox:', bbox); // [west, south, east, north]
```

### CommonJS Support

The package also supports CommonJS:

```javascript
const { convert, TileSource, TileServer, TileCoord } = require('@versatiles/versatiles-rs');
```

## API Reference

### `convert(input, output, options?, onProgress?, onMessage?)`

Convert tiles from one format to another.

**Parameters:**

- `input` (string): Input file path (.versatiles, .mbtiles, .pmtiles, .tar, directory)
- `output` (string): Output file path
- `options` (ConvertOptions, optional):
  - `minZoom` (number): Minimum zoom level
  - `maxZoom` (number): Maximum zoom level
  - `bbox` (array): Bounding box `[west, south, east, north]`
  - `bboxBorder` (number): Border around bbox in tiles
  - `compress` (string): Compression `"gzip"`, `"brotli"`, or `"uncompressed"`
  - `flipY` (boolean): Flip tiles vertically
  - `swapXy` (boolean): Swap x and y coordinates
- `onProgress` (function, optional): Progress callback `(data: ProgressData) => void`
- `onMessage` (function, optional): Message callback `(data: MessageData) => void`

**Returns:** `Promise<void>`

### `class TileSource`

#### `TileSource.open(path)`

Open a tile container.

**Parameters:**

- `path` (string): File path or URL

**Returns:** `Promise<TileSource>`

#### `TileSource.fromVpl(vpl, basePath?)`

Create a tile source from VPL (VersaTiles Pipeline Language).

**Parameters:**

- `vpl` (string): VPL query string
- `basePath` (string, optional): Base path for resolving relative paths

**Returns:** `Promise<TileSource>`

#### `source.getTile(z, x, y)`

Get a single tile.

**Parameters:**

- `z` (number): Zoom level
- `x` (number): Tile column
- `y` (number): Tile row

**Returns:** `Promise<Buffer | null>`

#### `source.tileJson()`

Get TileJSON metadata.

**Returns:** `TileJSON`

```typescript
interface TileJSON {
  tilejson: string;
  tiles?: string[];
  vector_layers?: VectorLayer[];
  attribution?: string;
  bounds?: [number, number, number, number];
  center?: [number, number, number];
  // ... and more
}
```

#### `source.metadata()`

Get source metadata.

**Returns:** `SourceMetadata`

```typescript
interface SourceMetadata {
  tileFormat: string;
  tileCompression: string;
  minZoom: number;
  maxZoom: number;
}
```

#### `source.sourceType()`

Get source type information.

**Returns:** `SourceType`

#### `source.convertTo(output, options?, onProgress?, onMessage?)`

Convert this source to another format.

**Parameters:**

- `output` (string): Output file path
- `options` (ConvertOptions, optional): Same as `convert()`
- `onProgress` (function, optional): Progress callback
- `onMessage` (function, optional): Message callback

**Returns:** `Promise<void>`

### `class TileServer`

#### `new TileServer(options?)`

Create a new tile server.

**Parameters:**

- `options` (object, optional):
  - `ip` (string): IP address to bind (default: `"0.0.0.0"`)
  - `port` (number): Port number (default: `8080`)
  - `minimalRecompression` (boolean): Use minimal recompression

#### `server.addTileSourceFromPath(name, path)`

Add a tile source from a file path.

**Parameters:**

- `name` (string): Source name (URL will be `/tiles/{name}/...`)
- `path` (string): Container file path

**Returns:** `Promise<void>`

#### `server.addTileSource(name, source)`

Add a tile source from a TileSource instance.

**Parameters:**

- `name` (string): Source name
- `source` (TileSource): TileSource instance

**Returns:** `Promise<void>`

#### `server.removeTileSource(name)`

Remove a tile source.

**Parameters:**

- `name` (string): Source name to remove

**Returns:** `Promise<void>`

#### `server.addStaticSource(path, urlPrefix?)`

Add static file source.

**Parameters:**

- `path` (string): Directory or .tar file
- `urlPrefix` (string, optional): URL prefix (default: `"/"`)

**Returns:** `Promise<void>`

#### `server.start()`

Start the HTTP server.

**Returns:** `Promise<void>`

#### `server.stop()`

Stop the HTTP server.

**Returns:** `Promise<void>`

#### `server.port`

Get server port (getter).

**Returns:** `Promise<number>`

### `class TileCoord`

#### `new TileCoord(z, x, y)`

Create a tile coordinate.

**Parameters:**

- `z` (number): Zoom level
- `x` (number): Column
- `y` (number): Row

#### `TileCoord.fromGeo(lon, lat, z)`

Create from geographic coordinates (static).

**Parameters:**

- `lon` (number): Longitude
- `lat` (number): Latitude
- `z` (number): Zoom level

**Returns:** `TileCoord`

#### `coord.toGeo()`

Convert to geographic coordinates.

**Returns:** `[number, number]` - `[lon, lat]`

#### `coord.toGeoBbox()`

Get geographic bounding box.

**Returns:** `[number, number, number, number]` - `[west, south, east, north]`

#### `coord.toJson()`

Get JSON representation.

**Returns:** `string`

#### Properties

- `coord.z` (number): Zoom level
- `coord.x` (number): Column
- `coord.y` (number): Row

## Supported Formats

- **VersaTiles** (`.versatiles`) - Native format
- **MBTiles** (`.mbtiles`) - SQLite-based format
- **PMTiles** (`.pmtiles`) - Cloud-optimized format
- **TAR** (`.tar`) - Archive format
- **Directory** - File system based

## Examples

See the [examples](./examples) directory for more usage examples:

- [convert.ts](./examples/convert.ts) - Format conversion with various options
- [convert-with-progress.ts](./examples/convert-with-progress.ts) - Conversion with progress monitoring
- [probe.ts](./examples/probe.ts) - Container inspection
- [serve.ts](./examples/serve.ts) - HTTP tile server
- [read-tiles.ts](./examples/read-tiles.ts) - Reading tiles and coordinate conversion

All examples use TypeScript and can be run with:

```bash
npx tsx examples/<filename>.ts
```

## Development

### Building from Source

```bash
# Install dependencies
npm install

# Build debug version
npm run build:debug

# Build release version
npm run build

# Run tests
npm test
```

### Requirements

- Node.js >= 16
- Rust toolchain (for building from source)

## License

MIT License - see [LICENSE](../LICENSE) for details.

## Links

- [VersaTiles Documentation](https://docs.versatiles.org/)
- [VersaTiles Rust](https://github.com/versatiles-org/versatiles-rs)
- [Issue Tracker](https://github.com/versatiles-org/versatiles-rs/issues)

## Contributing

Contributions are welcome! Please see the main [versatiles-rs repository](https://github.com/versatiles-org/versatiles-rs) for contribution guidelines.
