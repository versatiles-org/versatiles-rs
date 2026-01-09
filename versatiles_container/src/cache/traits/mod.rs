//! Trait definitions for the VersaTiles caching subsystem.
//!
//! This module provides the [`CacheValue`] trait for serializing and
//! deserializing values to/from the disk cache backend.

mod value;

pub use value::*;
