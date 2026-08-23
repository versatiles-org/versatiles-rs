//! Compressing and decompressing tile data.
//!
//! Covers the three codecs tile containers use — gzip, Brotli and Zstd — behind
//! one `TileCompression` enum, plus the machinery for deciding *which* to use:
//! [`TargetCompression`] describes what a consumer will accept, and
//! [`CompressionGoal`] whether to optimise for speed or size.
//!
//! Decompression is bounded by [`max_decompressed_bytes`], which stops a small
//! hostile input from expanding into an out-of-memory abort.

mod compression_goal;
mod functions;
mod limit;
mod methods;
mod target_compression;
#[cfg(test)]
pub mod test_utils;

pub use compression_goal::*;
pub use functions::*;
pub use limit::*;
pub use methods::*;
pub use target_compression::*;
