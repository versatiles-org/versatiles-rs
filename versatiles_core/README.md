# versatiles_core

Core types and utilities for the VersaTiles ecosystem.

[![Crates.io](https://img.shields.io/crates/v/versatiles_core)](https://crates.io/crates/versatiles_core)
[![Documentation](https://docs.rs/versatiles_core/badge.svg)](https://docs.rs/versatiles_core)

## Overview

`versatiles_core` provides the foundational types and utilities used throughout the VersaTiles tile processing ecosystem. It includes coordinate systems (tile coordinates, bounding boxes), format type definitions, byte iteration utilities, and tile traversal helpers.

This crate serves as the base dependency for all other VersaTiles components.

## Features

- **Coordinate Types**: `TileCoord`, `TileBBox`, `TileBBoxPyramid` for working with tile coordinates and bounding boxes
- **Format Definitions**: Type-safe enums for tile formats (`TileFormat`), compressions (`TileCompression`), and precompressions
- **Byte Utilities**: Efficient `ByteIterator` for reading blob data
- **Traversal**: Tools for iterating through tile pyramids and bounding boxes
- **I/O Utilities**: Helper traits and types for working with tile data streams

## Usage

```sh
cargo add versatiles_core
```

Or see [crates.io/crates/versatiles_core](https://crates.io/crates/versatiles_core) for version info and [docs.rs/versatiles_core](https://docs.rs/versatiles_core) for API documentation.

### Example

```rust
use versatiles_core::{TileCoord, TileBBox, TileBBoxPyramid};

// Create a tile coordinate (zoom, x, y)
let coord = TileCoord::new(5, 16, 10)?;

// Create a bounding box at a specific zoom level
let bbox = TileBBox::new(5, 10, 12, 15, 20)?;

// Create a pyramid of bounding boxes across zoom levels
let pyramid = TileBBoxPyramid::new_full(8);

// Convert coordinates
let geo_bbox = bbox.get_geo_bbox();
println!("Geographic bounds: {:?}", geo_bbox);
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_core](https://docs.rs/versatiles_core).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
