# versatiles_image

Image processing and codec support for VersaTiles.

[![Crates.io](https://img.shields.io/crates/v/versatiles_image)](https://crates.io/crates/versatiles_image)
[![Documentation](https://docs.rs/versatiles_image/badge.svg)](https://docs.rs/versatiles_image)

## Overview

`versatiles_image` provides utilities and trait extensions for working with raster images in the VersaTiles ecosystem. It offers a unified interface for encoding, decoding, and transforming images across multiple formats.

This crate standardizes image operations used throughout the VersaTiles tile processing pipeline.

## Features

- **Multiple Codecs**: Support for PNG, JPEG, WEBP, and AVIF formats
- **Unified API**: Consistent interface built on `image::DynamicImage`
- **Format Conversion**: Transcode between different image formats
- **Image Operations**: Scale, crop, flatten, and transform images
- **Metadata Access**: Query image dimensions, color types, and format information
- **Test Utilities**: Generate deterministic test images for development

## Usage

```sh
cargo add versatiles_image
```

Or see [crates.io/crates/versatiles_image](https://crates.io/crates/versatiles_image) for version info and [docs.rs/versatiles_image](https://docs.rs/versatiles_image) for API documentation.

### Example

```rust
use versatiles_core::TileFormat;
use versatiles_image::{
    DynamicImage, DynamicImageTraitConvert, DynamicImageTraitOperation, GenericImageView, decode, encode,
};

fn main() -> anyhow::Result<()> {
    let image = DynamicImage::new_rgb8(512, 512);

    // Encode and decode; `quality` and `effort` are ignored by codecs without them
    let png = encode(&image, TileFormat::PNG, None, Some(6))?;
    let webp = image.to_blob(TileFormat::WEBP, Some(80), None)?;
    let decoded = decode(&png, TileFormat::PNG)?;
    assert_eq!(decoded.dimensions(), (512, 512));

    // Transform: halve the size, then cut out a 128×128 corner
    let half = decoded.scaled_down(2)?;
    let corner = half.extract(0.0, 0.0, 128.0, 128.0, 128, 128)?;
    assert_eq!(corner.dimensions(), (128, 128));

    println!("PNG {} bytes, WebP {} bytes", png.len(), webp.len());
    Ok(())
}
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_image](https://docs.rs/versatiles_image).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
