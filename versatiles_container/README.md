# versatiles_container

Read, convert, and write tile containers for VersaTiles.

[![Crates.io](https://img.shields.io/crates/v/versatiles_container)](https://crates.io/crates/versatiles_container)
[![Documentation](https://docs.rs/versatiles_container/badge.svg)](https://docs.rs/versatiles_container)

## Overview

`versatiles_container` provides the I/O layer for working with map tile containers in multiple formats. It offers a unified interface for reading from and writing to various tile storage formats through a registry-based system with runtime composition.

This crate is designed for flexibility: readers are object-safe and can be wrapped by adapters (bbox filters, axis flips, compression overrides) before being written with the appropriate writer.

## Supported Formats

- **`.versatiles`**: Native VersaTiles container format
- **`.mbtiles`**: MBTiles (SQLite-based)
- **`.pmtiles`**: PMTiles (cloud-optimized)
- **`.tar`**: TAR archives
- **Directories**: Tile files in directory structures

## Features

- **Format Registry**: Automatic reader/writer selection based on file extension
- **Stream Processing**: Efficient tile streaming with minimal memory overhead
- **Runtime Composition**: Chain adapters to transform tile streams (filtering, compression, coordinate transforms)
- **Caching**: Built-in tile and metadata caching
- **Progress Tracking**: Monitor conversion progress with event bus

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
versatiles_container = "2.3"
versatiles_core = "2.3"
```

### Example

```rust
use versatiles_container::*;
use versatiles_core::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Open a source container via the registry
    let runtime = TilesRuntime::default();
    let reader = runtime.get_reader_from_str("input.mbtiles").await?;

    // Optionally adapt the reader: limit to a bbox pyramid
    let params = TilesConverterParameters {
        bbox_pyramid: Some(TileBBoxPyramid::new_full(8)),
        ..Default::default()
    };
    let reader = Box::new(TilesConvertReader::new_from_reader(reader, params)?);

    // Write to a target path; format is inferred from the extension
    runtime.write_to_path(reader, "output.versatiles").await?;
    Ok(())
}
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_container](https://docs.rs/versatiles_container).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
