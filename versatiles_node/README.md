# @versatiles/versatiles

Node.js bindings for [VersaTiles](https://github.com/versatiles-org/versatiles-rs) - convert, serve, and process map tiles in various formats.

## Features

- 🚀 **Fast & Native** - Powered by Rust with zero-copy operations
- 🔄 **Format Conversion** - Convert between MBTiles, PMTiles, VersaTiles, TAR, and directories
- 🗺️ **Tile Server** - Built-in HTTP tile server with dynamic source management
- 📊 **Metadata Access** - Read TileJSON and inspect container details
- 🌍 **Coordinate Utils** - Convert between tile and geographic coordinates
- ⚡ **Async API** - Non-blocking operations with Promise-based interface

## Installation

```bash
npm install @versatiles/versatiles
# or
yarn add @versatiles/versatiles
```

Pre-built binaries are available for:
- macOS (arm64, x64)
- Linux (x64, arm64, musl)
- Windows (x64)

## Quick Start

### Convert Tiles

```javascript
const { convertTiles } = require('@versatiles/versatiles');

await convertTiles('input.mbtiles', 'output.versatiles', {
  minZoom: 0,
  maxZoom: 14,
  bbox: [-180, -85, 180, 85],
  compress: 'gzip'
});
```

### Serve Tiles

```javascript
const { TileServer } = require('@versatiles/versatiles');

const server = new TileServer({ port: 8080 });
await server.addTileSource('osm', 'tiles.mbtiles');
await server.start();

console.log(`Server running at http://localhost:${await server.port}`);
```

### Read Tiles

```javascript
const { ContainerReader } = require('@versatiles/versatiles');

const reader = await ContainerReader.open('tiles.mbtiles');

// Get a single tile
const tile = await reader.getTile(5, 16, 10);
if (tile) {
  console.log('Tile size:', tile.length, 'bytes');
}

// Get metadata
const tileJSON = JSON.parse(await reader.tileJSON);
console.log('Format:', tileJSON.tile_format);

const params = await reader.parameters;
console.log('Zoom levels:', params.minZoom, '-', params.maxZoom);
```

### Probe Container

```javascript
const { probeTiles } = require('@versatiles/versatiles');

const info = await probeTiles('tiles.mbtiles');
console.log('Container:', info.containerName);
console.log('Source:', info.sourceName);
console.log('Format:', info.parameters.tileFormat);
console.log('Compression:', info.parameters.tileCompression);
```

### Coordinate Conversion

```javascript
const { TileCoord } = require('@versatiles/versatiles');

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

## API Reference

### `convertTiles(input, output, options?)`

Convert tiles from one format to another.

**Parameters:**
- `input` (string): Input file path (.versatiles, .mbtiles, .pmtiles, .tar, directory)
- `output` (string): Output file path
- `options` (object, optional):
  - `minZoom` (number): Minimum zoom level
  - `maxZoom` (number): Maximum zoom level
  - `bbox` (array): Bounding box `[west, south, east, north]`
  - `bboxBorder` (number): Border around bbox in tiles
  - `compress` (string): Compression `"gzip"`, `"brotli"`, or `"uncompressed"`
  - `flipY` (boolean): Flip tiles vertically
  - `swapXy` (boolean): Swap x and y coordinates

**Returns:** `Promise<void>`

### `probeTiles(path, depth?)`

Inspect a tile container.

**Parameters:**
- `path` (string): Container file path
- `depth` (string, optional): Probe depth - currently not implemented

**Returns:** `Promise<ProbeResult>`

```typescript
interface ProbeResult {
  sourceName: string;
  containerName: string;
  tileJson: string;
  parameters: ReaderParameters;
}
```

### `class ContainerReader`

#### `ContainerReader.open(path)`

Open a tile container.

**Parameters:**
- `path` (string): File path or URL

**Returns:** `Promise<ContainerReader>`

#### `reader.getTile(z, x, y)`

Get a single tile.

**Parameters:**
- `z` (number): Zoom level
- `x` (number): Tile column
- `y` (number): Tile row

**Returns:** `Promise<Buffer | null>`

#### `reader.tileJSON`

Get TileJSON metadata (getter).

**Returns:** `Promise<string>`

#### `reader.parameters`

Get reader parameters (getter).

**Returns:** `Promise<ReaderParameters>`

```typescript
interface ReaderParameters {
  tileFormat: string;
  tileCompression: string;
  minZoom: number;
  maxZoom: number;
}
```

#### `reader.sourceName`

Get source name (getter).

**Returns:** `Promise<string>`

#### `reader.containerName`

Get container type (getter).

**Returns:** `Promise<string>`

#### `reader.convertTo(output, options?)`

Convert this container to another format.

**Parameters:**
- `output` (string): Output file path
- `options` (ConvertOptions, optional): Same as `convertTiles`

**Returns:** `Promise<void>`

#### `reader.probe(depth?)`

Probe container details.

**Parameters:**
- `depth` (string, optional): Probe depth

**Returns:** `Promise<ProbeResult>`

### `class TileServer`

#### `new TileServer(options?)`

Create a new tile server.

**Parameters:**
- `options` (object, optional):
  - `ip` (string): IP address to bind (default: `"0.0.0.0"`)
  - `port` (number): Port number (default: `8080`)
  - `minimalRecompression` (boolean): Use minimal recompression

#### `server.addTileSource(name, path)`

Add a tile source.

**Parameters:**
- `name` (string): Source name (URL will be `/tiles/{name}/...`)
- `path` (string): Container file path

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
- [convert.js](./examples/convert.js) - Format conversion
- [probe.js](./examples/probe.js) - Container inspection
- [serve.js](./examples/serve.js) - Tile server
- [read-tiles.js](./examples/read-tiles.js) - Reading tiles

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
