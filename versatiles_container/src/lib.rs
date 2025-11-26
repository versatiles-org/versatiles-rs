//! VersaTiles Container: read, convert, and write tile containers.
//!
//! This crate exposes a small set of building blocks to work with map tile containers:
//! - a registry that maps file extensions to readers/writers,
//! - reader traits and adapters to stream tiles,
//! - writer traits to serialize tiles,
//! - utilities like caching and streaming combinators.
//!
//! It is designed for **runtime composition**: readers are object‑safe and can be wrapped
//! by adapters (e.g. bbox filters, axis flips, compression overrides) and then written
//! out with the appropriate writer inferred from the output path.
//!
//! # Quick start
//! ```rust
//! use versatiles_container::*;
//! use versatiles_core::*;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Open a source container via the registry
//!     let registry = ContainerRegistry::default();
//!     let reader = registry.get_reader_from_str("../testdata/berlin.mbtiles").await?;
//!
//!     // Optionally adapt the reader: limit to a bbox pyramid, keep compression as-is
//!     let params = TilesConverterParameters {
//!         bbox_pyramid: Some(TileBBoxPyramid::new_full(8)),
//!         ..Default::default()
//!     };
//!     let reader = Box::new(TilesConvertReader::new_from_reader(reader, params)?);
//!
//!     // Write to a target path; format is inferred from the extension
//!     let output = std::env::temp_dir().join("example.versatiles");
//!     registry.write_to_path(reader, &output).await?;
//!     Ok(())
//! }
//! ```
//!
//! # Features
//! - `cli`: enables human‑readable probing of containers and tiles.
//! - `test`: helpers for integration tests in downstream crates.
//!
//! ## See also
//! - [`ContainerRegistry`]: register custom reader/writer implementations at runtime
//! - [`TilesReaderTrait`], [`TilesWriterTrait`]: object‑safe traits for IO
//! - [`TilesConvertReader`], [`convert_tiles_container`]: convenience conversion helpers

mod cache;
/// Re‑exports in‑memory caches and helpers used by readers/writers.
pub use cache::*;

mod container;
/// Re‑exports the container registry and common open/write helpers.
pub use container::*;

mod types;
/// Re‑exports reader/writer traits, converters, and auxiliary types.
pub use types::*;
