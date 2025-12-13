# Source Directory

This directory contains both the Rust source code and TypeScript integration tests.

## Structure

```
src/
├── lib.rs               # Main Rust module (napi entry point)
├── container.rs         # ContainerReader implementation
├── server.rs            # TileServer implementation  
├── types.rs             # TileCoord and type definitions
├── utils.rs             # Error conversion utilities
├── container.test.ts    # ContainerReader tests
├── server.test.ts       # TileServer tests
├── types.test.ts        # TileCoord tests
└── functions.test.ts    # Standalone function tests
```

## Running Tests

The TypeScript tests are co-located with the Rust code for easier navigation and maintenance.

```bash
# Run all tests
npm test

# Run specific test file
npx tsx --test src/container.test.ts
npx tsx --test src/server.test.ts
npx tsx --test src/types.test.ts
npx tsx --test src/functions.test.ts
```

## Test Coverage

- **93 tests total**
- **87 passing (93.5%)**
- **6 failing (known server HTTP routing limitation)**

### Test Files

#### container.test.ts (17 tests)
Tests for `ContainerReader` class:
- Opening various formats (MBTiles, PMTiles)
- Reading tiles
- TileJSON and metadata access
- Probing containers
- Converting between formats

#### server.test.ts (17 tests)
Tests for `TileServer` class:
- Server lifecycle (start/stop/restart)
- Adding tile and static sources
- HTTP endpoints
- Port management

#### types.test.ts (37 tests)
Tests for `TileCoord` class:
- Coordinate creation and validation
- Geo ↔ tile conversion
- Round-trip conversions
- Edge cases

#### functions.test.ts (24 tests)
Tests for standalone functions:
- `probeTiles()` - Container probing
- `convertTiles()` - Format conversion with various options

## Writing Tests

Tests use Node.js's built-in test runner with TypeScript via `tsx`:

```typescript
import { describe, test } from 'node:test';
import assert from 'node:assert';
import { ContainerReader } from '../index.js';

describe('MyFeature', () => {
  test('should work', async () => {
    const reader = await ContainerReader.open('path/to/file.mbtiles');
    const tile = await reader.getTile(5, 17, 10);
    assert.ok(tile);
  });
});
```

## NPM Package

Test files (`*.test.ts`) are excluded from the published NPM package via `.npmignore`. Only the compiled bindings and type definitions are included.
