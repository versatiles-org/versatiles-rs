# versatiles_image

Image processing and codec support for VersaTiles.

[![Crates.io](https://img.shields.io/crates/v/versatiles_image)](https://crates.io/crates/versatiles_image)
[![Documentation](https://docs.rs/versatiles_image/badge.svg)](https://docs.rs/versatiles_image)

## Overview

`versatiles_image` provides utilities and trait extensions for working with raster images in the VersaTiles ecosystem. It offers a unified interface for encoding, decoding, and transforming images across multiple formats.

This crate standardizes image operations used throughout the VersaTiles tile processing pipeline.

## Features

- **Multiple Codecs**: Support for PNG, JPEG, WEBP, and AVIF formats (AVIF for encoding only)
- **Unified API**: Consistent interface built on `image::DynamicImage`
- **Format Conversion**: Transcode between different image formats
- **Image Operations**: Scale, crop, flatten, and transform images
- **Metadata Access**: Query image dimensions, color types, and format information
- **Test Utilities**: Generate deterministic test images for development

## Format support

What each codec does, and what the `quality` and `effort` arguments of `encode` mean for it:

|                  | PNG               | JPEG                      | WebP                    | AVIF                             |
| ---------------- | ----------------- | ------------------------- | ----------------------- | -------------------------------- |
| Decoding         | yes               | yes                       | yes                     | **no**                           |
| Encoding         | yes               | yes                       | yes                     | yes                              |
| Lossless         | always            | no                        | at `quality` 100        | no                               |
| Lossy            | no                | always                    | at `quality` below 100  | always                           |
| Transparency     | yes               | no                        | yes                     | yes                              |
| `quality`        | ignored           | 0–99, default 95          | 0–100, default 95       | 0–99, default 92                 |
| `effort`         | 0–100, default 90 | ignored                   | 0–100, default 80       | 0–100, default encoder speed 8   |
| Encoding library | `image` crate     | `image` crate             | libwebp (`libwebp-sys`) | ravif, through the `image` crate |
| Decoding library | `image` crate     | `image` crate (zune-jpeg) | libwebp (`libwebp-sys`) | none                             |

Which pixel layouts each encoder accepts:

| Input layout   | PNG | JPEG           | WebP           | AVIF |
| -------------- | --- | -------------- | -------------- | ---- |
| Grey           | yes | yes            | stored as RGB  | yes  |
| Grey and alpha | yes | only if opaque | stored as RGBA | yes  |
| RGB            | yes | yes            | yes            | yes  |
| RGBA           | yes | only if opaque | yes            | yes  |
| 16-bit, float  | no  | no             | no             | no   |

Details:

- **Opaque alpha is dropped.** PNG, JPEG and WebP remove an alpha channel that has no transparent pixel before encoding, which saves bytes and loses nothing.
- **Translucent images can't become JPEG.** The encoder returns an error rather than picking a background. Flatten the image onto a colour first, for example with `into_flattened`.
- **Lossy WebP covers alpha too.** Below quality 100 the alpha channel is compressed lossily, like the colours. Only quality 100 keeps transparency exact.
- **Decoding gives 8-bit images.** A 16-bit PNG is narrowed to 8 bits per channel, since every encoder here takes 8-bit only. Palette PNGs decode as RGB or RGBA, and JPEG decodes greyscale, CMYK and progressive files.
- **PNG is never written with a palette.** A palette PNG that is decoded and encoded again comes out as truecolour, which can make it larger.
- **Decoding is bounded.** An image may be at most 16,384 pixels on each side and decode to at most 512 MiB. Larger ones are refused from their header, before anything is allocated.
- **AVIF can't be read back.** Tiles encoded as AVIF can be stored and served, but nothing that needs their pixels can process them again.

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
