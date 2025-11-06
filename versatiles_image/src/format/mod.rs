//! This module defines and re-exports image format handlers (AVIF, JPEG, PNG, WebP).
//! The `all` module provides shared traits and helper utilities for working with multiple image formats.
//! Each submodule implements decoding and encoding logic for its respective image type.

mod all;

pub mod avif;
pub mod jpeg;
pub mod png;
pub mod webp;
pub use all::*;
