# versatiles_geometry

Geometric data structures and utilities for the VersaTiles ecosystem.

[![Crates.io](https://img.shields.io/crates/v/versatiles_geometry)](https://crates.io/crates/versatiles_geometry)
[![Documentation](https://docs.rs/versatiles_geometry/badge.svg)](https://docs.rs/versatiles_geometry)

## Overview

`versatiles_geometry` provides the geometric data handling layer for VersaTiles, including primitives for working with points, lines, and polygons, as well as support for GeoJSON and Mapbox Vector Tiles (MVT).

This crate is essential for reading, transforming, and exporting geospatial vector data.

## Features

- **Geometry Primitives**: Built on the `geo-types` `Point`, `LineString`, `Polygon` and `MultiPolygon`, with features, properties and collections in `geo`
- **GeoJSON Support**: Parse and serialize GeoJSON and newline-delimited GeoJSON (NDGeoJSON)
- **Vector Tiles**: Read and write Mapbox Vector Tile (MVT) protobuf format, with full MVT 2.1 validation and repair
- **MVT Validation**: `validate_tile` checks for missing `extent`/`version` fields, duplicate layer names, polygon winding issues, and degenerate rings
- **MVT Repair**: `repair_tile` fixes every issue the validator reports in one pass, with an option to drop undecodable features
- **Tile Outlines**: Generate polygonal outlines from tile bounding boxes
- **Transformations**: Convert between different geometric representations

## Usage

```sh
cargo add versatiles_geometry
```

Or see [crates.io/crates/versatiles_geometry](https://crates.io/crates/versatiles_geometry) for version info and [docs.rs/versatiles_geometry](https://docs.rs/versatiles_geometry) for API documentation.

### Example

```rust
use versatiles_geometry::{
    geojson::parse_geojson,
    vector_tile::{VectorTile, VectorTileLayer, repair_tile, validate_tile},
};

fn main() -> anyhow::Result<()> {
    // Parse GeoJSON into features
    let collection = parse_geojson(
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[13.4,52.5]},"properties":{}}
        ]}"#,
    )?;
    assert_eq!(collection.features.len(), 1);

    // Encode a vector tile (MVT) and read it back
    let tile = VectorTile::new(vec![VectorTileLayer::new_standard("places")]);
    let blob = tile.to_blob()?;
    let tile = VectorTile::from_blob(&blob)?;

    // Check it against the MVT 2.1 spec, and repair it if needed.
    // `true` would also drop features that cannot be decoded.
    if !validate_tile(&tile).is_empty() {
        let _repaired = repair_tile(tile, false)?;
    }
    Ok(())
}
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_geometry](https://docs.rs/versatiles_geometry).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
