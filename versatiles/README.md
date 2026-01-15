# versatiles

A Rust library and CLI for reading, writing, and converting map tiles.

[![Crates.io](https://img.shields.io/crates/v/versatiles)](https://crates.io/crates/versatiles)
[![Documentation](https://docs.rs/versatiles/badge.svg)](https://docs.rs/versatiles)

## Overview

VersaTiles provides both a command-line interface and a Rust library for working with map tile containers in various formats including MBTiles, PMTiles, VersaTiles, TAR, and directory structures.

## As a CLI Tool

For CLI usage, installation instructions, command documentation, and production deployment guides, see the main [VersaTiles README](https://github.com/versatiles-org/versatiles-rs).

The CLI provides commands for:
- **`convert`**: Convert between tile formats
- **`probe`**: Inspect tile containers
- **`serve`**: Run an HTTP tile server
- **`dev`**: Development server with hot reload

## As a Library

The `versatiles` crate can be used as a library to integrate tile processing into your Rust applications.

### Installation

```sh
cargo add versatiles
```

Or see [crates.io/crates/versatiles](https://crates.io/crates/versatiles) for version info and [docs.rs/versatiles](https://docs.rs/versatiles) for API documentation.

### Example

```rust
use versatiles::{
    container::*,
    core::*,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let runtime = TilesRuntime::default();
    let reader = runtime.get_reader_from_str("input.pmtiles").await?;

    // Define the output filename
    let output_path = std::env::temp_dir().join("output.versatiles");

    // Write the tiles to the output file
    runtime.write_to_path(reader, &output_path).await?;

    println!("Tiles converted successfully!");
    Ok(())
}
```

## Features

- **`cli`** (default): Command-line interface
- **`server`** (default): HTTP tile server
- **`gdal`** (optional): GDAL raster support for reading GeoTIFF and other raster formats

## Component Crates

VersaTiles is built from several focused crates that can be used independently:

- [`versatiles_core`](https://crates.io/crates/versatiles_core): Core types and utilities (coordinates, formats, traversal)
- [`versatiles_container`](https://crates.io/crates/versatiles_container): Tile container I/O (read, write, convert)
- [`versatiles_geometry`](https://crates.io/crates/versatiles_geometry): Geometric data structures (GeoJSON, MVT)
- [`versatiles_image`](https://crates.io/crates/versatiles_image): Image processing (PNG, JPEG, WEBP, AVIF)
- [`versatiles_pipeline`](https://crates.io/crates/versatiles_pipeline): Tile processing pipelines (VPL language)
- [`versatiles_derive`](https://crates.io/crates/versatiles_derive): Procedural macros (internal use)

## Documentation

- **Library API**: [docs.rs/versatiles](https://docs.rs/versatiles)
- **CLI Documentation**: [GitHub README](https://github.com/versatiles-org/versatiles-rs)
- **Full Documentation**: [docs.versatiles.org](https://docs.versatiles.org/)

## Supported Formats

- `.versatiles` - Native VersaTiles container format
- `.mbtiles` - MBTiles (SQLite-based)
- `.pmtiles` - PMTiles (cloud-optimized)
- `.tar` - TAR archives
- Tile directories

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
