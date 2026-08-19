# @versatiles/versatiles-rs

Node.js bindings for [VersaTiles](https://github.com/versatiles-org/versatiles-rs) - convert, serve, and process map tiles in various formats.

## Features

- 🚀 **Fast & Native** - Powered by Rust with zero-copy operations
- 🔄 **Format Conversion** - Convert between MBTiles, PMTiles, VersaTiles, TAR, and directories
- 🗺️ **Tile Server** - Built-in HTTP tile server with dynamic source management
- 📊 **Metadata Access** - Read TileJSON and inspect container details
- 🌍 **Coordinate Utils** - Convert between tile and geographic coordinates
- 🧩 **VPL Pipelines** - Build, parse and losslessly edit VersaTiles Pipeline Language
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

console.log(`Server running at http://localhost:${server.port}`);
```

### Read Tiles

```javascript
import { TileSource } from '@versatiles/versatiles-rs';

const source = await TileSource.fromPath('tiles.mbtiles');

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

const source = await TileSource.fromPath('tiles.mbtiles');
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
- `output` (string): Output file path, or an `sftp://` URL
- `options` (ConvertOptions, optional):
  - `minZoom` (number): Minimum zoom level
  - `maxZoom` (number): Maximum zoom level
  - `bbox` (array): Bounding box `[west, south, east, north]`
  - `bboxBorder` (number): Border around bbox in tiles
  - `compress` (string): Compression `"gzip"`, `"brotli"`, or `"uncompressed"`
  - `flipY` (boolean): Flip tiles vertically
  - `swapXy` (boolean): Swap x and y coordinates
  - `writerOptions` (object): Format-specific settings for the writer, see below
  - `sshIdentity` (string): Private key file for an `sftp://` input, see below
- `onProgress` (function, optional): Progress callback `(data: ProgressData) => void`
- `onMessage` (function, optional): Message callback `(data: MessageData) => void`

**Returns:** `Promise<void>`

#### Writer options

`writerOptions` passes settings to the writer that produces `output`. Which ones exist depends on the
output format, so the keys keep the writer's own `snake_case` spelling rather than the camelCase of
the options above. An option the chosen format does not accept is an error that names what it does
accept — never a silent no-op.

PMTiles is the only format with writer options today:

| Key                 | Value   | Effect                                                                                                                |
| ------------------- | ------- | --------------------------------------------------------------------------------------------------------------------- |
| `allow_unclustered` | boolean | Write the archive in a single pass without physical clustering, at the cost of more range requests when serving it    |
| `reorder`           | boolean | Stay clustered by writing the tile data twice, at the cost of a second pass and temporary disk the size of the output |
| `temp_dir`          | path    | Where `reorder` puts that temporary file (default: the output file's own directory)                                   |

Both opt-ins matter only for a source that cannot supply Hilbert order — a pipeline containing
`raster_overview`, for example, builds lower zoom levels from higher ones and so produces tiles in
the opposite order. Writing such a source to `.pmtiles` without one of them fails; asking for both is
an error, since they buy different things. Booleans are strings: `'true'`, `'1'` or `'yes'`,
case-insensitively, and likewise for false.

```javascript
await convert('pipeline.vpl', 'output.pmtiles', {
  writerOptions: { reorder: 'true', temp_dir: '/mnt/scratch' },
});
```

#### SSH authentication

Reading from `sftp://` needs a key. Keys are files on disk: `sshIdentity` is the path to a private
key file, never the key itself.

The identity is resolved in this order, first match wins:

1. `sshIdentity` in the options passed to the call
2. The `VERSATILES_SSH_IDENTITY` environment variable, the same one the CLI reads
3. Nothing — a password in the URL (`sftp://user:pass@host/path`), the SSH agent, `~/.ssh/config`
   and the usual default key files still apply

```javascript
// One key for this source, whatever the environment says
const source = await TileSource.fromPath('sftp://host/tiles.pmtiles', {
  sshIdentity: '/home/deploy/.ssh/id_ed25519',
});

// Same option during a conversion, for an sftp:// input
await convert('sftp://host/tiles.mbtiles', 'local.pmtiles', {
  sshIdentity: '/home/deploy/.ssh/id_ed25519',
});
```

An `sftp://` URL works as a destination too, with the same identity resolution:

```javascript
await convert('local.mbtiles', 'sftp://host/tiles/berlin.pmtiles', {
  sshIdentity: '/home/deploy/.ssh/id_ed25519',
});
```

Uploading needs a format that can be written as a stream, which today means `.versatiles` and
`.pmtiles`. `.mbtiles` is a local SQLite file, and is refused with a message saying so.

### `class TileSource`

#### `TileSource.fromPath(path, options?)`

Open a tile container.

**Parameters:**

- `path` (string): File path or URL
- `options` (SourceOptions, optional):
  - `sshIdentity` (string): Private key file for an `sftp://` path, see [SSH authentication](#ssh-authentication)

**Returns:** `Promise<TileSource>`

#### `TileSource.openVpl(vpl, basePath?, options?)`

Create a tile source from VPL (VersaTiles Pipeline Language).

**Parameters:**

- `vpl` (string): VPL query string
- `basePath` (string, optional): Base path for resolving relative paths
- `options` (SourceOptions, optional): Same as `TileSource.fromPath()`

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

- `output` (string): Output file path, or an `sftp://` URL
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

**Returns:** `number`

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

### VPL — the VersaTiles Pipeline Language

Everything below is exported from `@versatiles/versatiles-rs/vpl` and shares one grammar with the
Rust engine, so anything it writes is something the engine will read.

#### Building and running a pipeline

```javascript
import { VPL } from '@versatiles/versatiles-rs/vpl';

const pipeline = VPL.fromContainer({ filename: 'world.versatiles' })
  .filter({ levelMin: 0, levelMax: 14 })
  .rasterFormat({ format: 'webp', quality: '80' });

pipeline.toString();
// "from_container filename=world.versatiles | filter level_max=14 level_min=0 | raster_format format=webp quality=80"

// resolve relative filenames against a directory, then execute
const source = await pipeline.fromPath('./data');
```

Values are quoted by the grammar rather than by the caller, so spaces, quotes and newlines need no
special handling. Note that parameters come out in alphabetical order: the pipeline a `VPL` object
describes is a set of parameters, not a sequence of them.

#### Parsing

```javascript
const pipeline = VPL.parse("from_container filename='world.versatiles'");

// Errors as data, for an editor that re-parses on every keystroke:
const error = VPL.parseError('from_container filename=a | vector_filter zoom');
error.span; // { start: 46, end: 46 } — byte offsets into the text
error.message; // "expected '=', got end of input"
error.trace; // the caret-annotated rendering the CLI prints
```

`VPL.parse` throws a `SyntaxError` instead, with the same object attached as `error.vpl`.

Offsets are **bytes**, not characters, so a caller can convert to whatever unit it counts in — an
editor in characters, a language server in UTF-16 units. Anything non-ASCII earlier in the line
makes the two differ.

#### Editing a file somebody wrote

`VPL` is the _semantic_ pipeline: it forgets comments, formatting and parameter order, which is
right for building a pipeline and wrong for saving one. To edit a `.vpl` file without rewriting the
parts nobody touched, use the concrete syntax tree, which keeps every byte:

```javascript
import { parseCst, stringifyCst } from '@versatiles/versatiles-rs/vpl';

const source = "# where the data lives\nfrom_container  filename = 'berlin.versatiles'\n";
const cst = parseCst(source);

stringifyCst(cst) === source; // true — nothing was lost on the way in

const value = cst.pipeline.nodes[0].value.properties[0].value;
value.token.text = 'hamburg.versatiles';
value.quote = 'bare';

stringifyCst(cst);
// "# where the data lives\nfrom_container  filename = hamburg.versatiles\n"
```

The comment, the double space and the spaces around `=` all survive, because only the bytes you
changed are rewritten. `parseCstResult` returns the error as data where `parseCst` throws.

Every token carries a `span` of byte offsets into the parsed text, and `leading` holds the
whitespace and comments before it. Both may be omitted when building a tree by hand — the minimum
is `{ pipeline: { nodes: [{ value: { name: { text: 'from_debug' } } }] } }`.

One caveat: `span` describes where a token **was**. After an edit the spans are stale; parse the
printed text again to get fresh ones.

To run an edited tree, print it and hand it to `TileSource.fromVpl(stringifyCst(cst))`.

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
- [vpl-pipeline.ts](./examples/vpl-pipeline.ts) - Building VPL pipelines programmatically

All examples use TypeScript and can be run with:

```bash
npx tsx examples/<filename>.ts
```

## Development

### Requirements

- Node.js >= 16
- Rust toolchain (for building from source)

### Build Process

The package has two outputs: a **native N-API module** (Rust compiled to a `.node` binary) and a **VPL TypeScript library** (generated from Rust operation metadata, then compiled to JS). The VPL generation step calls into the native module, so it must be built first.

```mermaid
flowchart TD
    subgraph "1. Native Module"
        RS[Rust source] -->|"npm run build:cjs"| CJS["index.cjs"]
        RS -->|"npm run build:esm"| ESM["index.js<br/>index.d.ts"]
    end

    subgraph "2. VPL Library"
        ESM -->|"npm run build:vpl"| VJS["vpl.js<br/>vpl.d.ts"]
    end
```

| Step             | Script      | What it does                                                                                                                                                       |
| ---------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Build native CJS | `build:cjs` | Compiles Rust to a native `.node` binary and generates `index.cjs`                                                                                                 |
| Build native ESM | `build:esm` | Generates `index.js` and `index.d.ts` (ESM wrapper + type declarations)                                                                                            |
| Generate VPL TS  | `build:vpl` | Runs `scripts/generate-vpl.ts` which calls the native `generateVplTypescript()` function to produce `vpl.ts`, then compiles it with `tsc` to `vpl.js` + `vpl.d.ts` |

### Building from Source

```bash
# Install dependencies
npm install

# Build debug version (native module + VPL)
npm run build:debug

# Build release version (native module + VPL)
npm run build

# Run tests
npm test
```

## License

MIT License - see [LICENSE](../LICENSE) for details.

## Links

- [VersaTiles Documentation](https://docs.versatiles.org/)
- [VersaTiles Rust](https://github.com/versatiles-org/versatiles-rs)
- [Issue Tracker](https://github.com/versatiles-org/versatiles-rs/issues)

## Contributing

Contributions are welcome! Please see the main [versatiles-rs repository](https://github.com/versatiles-org/versatiles-rs) for contribution guidelines.
