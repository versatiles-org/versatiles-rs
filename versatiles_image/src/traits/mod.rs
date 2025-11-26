//! Core image trait extensions for VersaTiles.
//!
//! This module aggregates several traits that extend [`image::DynamicImage`] with additional
//! functionality used throughout the VersaTiles image pipeline:
//!
//! - [`DynamicImageTraitConvert`] — conversion between formats, raw buffers, and iteration over pixel bytes.
//! - [`DynamicImageTraitInfo`] — lightweight metadata, comparison, and introspection helpers.
//! - [`DynamicImageTraitOperation`] — higher‑level image manipulation (flattening, scaling, cropping, etc.).

mod convert;
mod info;
mod operation;
#[cfg(any(test, feature = "test"))]
mod test;

pub use convert::*;
pub use info::*;
pub use operation::*;
#[cfg(any(test, feature = "test"))]
pub use test::*;
