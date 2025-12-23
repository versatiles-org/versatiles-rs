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

Add this to your `Cargo.toml`:

```toml
[dependencies]
versatiles_image = "2.3"
```

### Example

```rust
use versatiles_image::{DynamicImage, ImageFormat, ImageConvert};

// Load an image
let img = DynamicImage::load_from_bytes(&image_data)?;

// Convert format
let png_data = img.encode_to_format(ImageFormat::PNG)?;
let webp_data = img.encode_to_format(ImageFormat::WEBP)?;

// Transform
let resized = img.resize(256, 256)?;
let cropped = img.crop(0, 0, 128, 128)?;

// Get metadata
let (width, height) = img.dimensions();
println!("Image size: {}x{}", width, height);
```

## API Documentation

For detailed API documentation, see [docs.rs/versatiles_image](https://docs.rs/versatiles_image).

## Part of VersaTiles

This crate is part of the [VersaTiles](https://github.com/versatiles-org/versatiles-rs) project, a toolbox for working with map tile containers in various formats.

For the complete toolset including CLI tools and servers, see the main [VersaTiles repository](https://github.com/versatiles-org/versatiles-rs).

## License

MIT License - see [LICENSE](https://github.com/versatiles-org/versatiles-rs/blob/main/LICENSE) for details.
