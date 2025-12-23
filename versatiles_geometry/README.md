# versatiles_geometry

Geometric data structures and utilities for the VersaTiles ecosystem.

[![Crates.io](https://img.shields.io/crates/v/versatiles_geometry)](https://crates.io/crates/versatiles_geometry)
[![Documentation](https://docs.rs/versatiles_geometry/badge.svg)](https://docs.rs/versatiles_geometry)

## Overview

`versatiles_geometry` provides the geometric data handling layer for VersaTiles, including primitives for working with points, lines, and polygons, as well as support for GeoJSON and Mapbox Vector Tiles (MVT).

This crate is essential for reading, transforming, and exporting geospatial vector data.

## Features

- **Geometry Primitives**: Core geometric types including `Point`, `LineString`, `Polygon`, and `MultiPolygon`
- **GeoJSON Support**: Parse and serialize GeoJSON and newline-delimited GeoJSON (NDGeoJSON)
- **Vector Tiles**: Read and write Mapbox Vector Tile (MVT) protobuf format
- **Tile Outlines**: Generate polygonal outlines from tile bounding boxes
- **Transformations**: Convert between different geometric representations

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
versatiles_geometry = "2.3"
```

### Example

```rust
use versatiles_geometry::{
    geo::{Point, Polygon},
    geojson::GeoJson,
    vector_tile::VectorTile,
};

// Create geometric primitives
let point = Point::new(13.4, 52.5);

// Parse GeoJSON
let geojson_str = r#"{"type": "Point", "coordinates": [13.4, 52.5]}"#;
let geojson = GeoJson::from_str(geojson_str)?;

// Work with vector tiles (MVT)
let mvt_data: Vec<u8> = /* ... */;
let tile = VectorTile::from_bytes(&mvt_data)?;
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_geometry](https://docs.rs/versatiles_geometry).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
