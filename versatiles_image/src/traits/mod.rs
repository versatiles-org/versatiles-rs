//! Core image trait extensions for VersaTiles.
//!
//! This module aggregates several traits that extend [`image::DynamicImage`] with additional
//! functionality used throughout the VersaTiles image pipeline:
//!
//! - [`convert`] — conversion between formats, raw buffers, and iteration over pixel bytes.
//! - [`info`] — lightweight metadata, comparison, and introspection helpers.
//! - [`operation`] — higher‑level image manipulation (flattening, scaling, cropping, etc.).
//! - [`test`] — synthetic image generators used in tests and benchmarks.

mod convert;
mod info;
mod operation;
mod test;

pub use convert::*;
pub use info::*;
pub use operation::*;
pub use test::*;
