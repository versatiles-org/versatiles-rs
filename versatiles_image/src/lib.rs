//! VersaTiles image processing crate.
//!
//! This crate provides utilities and trait extensions built around the
//! [`image::DynamicImage`] type. It standardizes image encoding/decoding and high‑level
//! operations used in the VersaTiles pipeline.
//!
//! ### Features
//! - Unified access to multiple codecs (`PNG`, `JPEG`, `WEBP`, `AVIF`).
//! - Trait extensions for:
//!   - Conversion and encoding (`traits::convert`)
//!   - Metadata and pixel introspection (`traits::info`)
//!   - Common transformations (scaling, flattening, cropping; `traits::operation`)
//!   - Deterministic test image generation (`traits::test`)

pub mod format;
pub mod traits;

pub use format::*;
pub use image::{DynamicImage, GenericImageView, ImageBuffer, Luma, LumaA, Rgb, Rgba};
pub use traits::*;
