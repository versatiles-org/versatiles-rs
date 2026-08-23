//! VersaTiles image processing crate.
//!
//! This crate provides utilities and trait extensions built around the
//! [`image::DynamicImage`] type. It standardizes image encoding/decoding and high‑level
//! operations used in the VersaTiles pipeline.
//!
//! ### Features
//! - Unified access to multiple codecs (`PNG`, `JPEG`, `WEBP`, `AVIF`).
//! - Color parsing utilities (`color`)
//! - Trait extensions for:
//!   - Conversion and encoding (`traits::convert`)
//!   - Metadata and pixel introspection (`traits::info`)
//!   - Common transformations (scaling, flattening, cropping; `traits::operation`)
//!   - Deterministic test image generation (`traits::test`)
//!
//! ### Encoding and decoding a tile
//!
//! [`encode`] takes `quality` and `effort` for every format and ignores the one
//! a given codec has no use for — PNG has no quality setting, so it is `None`
//! here:
//!
//! ```
//! use versatiles_core::TileFormat;
//! use versatiles_image::{DynamicImage, GenericImageView, decode, encode};
//!
//! let image = DynamicImage::new_rgb8(256, 256);
//!
//! let blob = encode(&image, TileFormat::PNG, None, Some(6))?;
//! let decoded = decode(&blob, TileFormat::PNG)?;
//!
//! assert_eq!(decoded.dimensions(), (256, 256));
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod color;
pub mod format;
pub mod traits;

pub use format::*;
pub use image::{DynamicImage, GenericImage, GenericImageView, ImageBuffer, Luma, LumaA, Pixel, Rgb, Rgba};
pub use traits::*;
