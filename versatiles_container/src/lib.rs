//! `VersaTiles` Container: read, convert, and write tile containers.
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
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Open a source container via the registry
//!     let runtime = TilesRuntime::default();
//!     let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
//!
//!     // Optionally adapt the reader: limit to a tile pyramid, keep compression as-is
//!     let params = TilesConverterParameters {
//!         tile_pyramid: Some(TilePyramid::new_full_up_to(8)),
//!         ..Default::default()
//!     };
//!     let reader = Arc::new(TilesConvertReader::new_from_reader(reader, params).await?);
//!
//!     // Write to a target path; format is inferred from the extension
//!     let output = std::env::temp_dir().join("example.versatiles");
//!     runtime.write_to_path(reader, &output).await?;
//!     Ok(())
//! }
//! ```
//!
//! # Four traits, two axes
//!
//! Reading and writing are each split in two, which is easier to hold onto as a
//! grid than as four separate names:
//!
//! |           | Open a container                            | Move the tiles                                   |
//! | --------- | ------------------------------------------- | ------------------------------------------------ |
//! | **Read**  | [`TilesReader`] — a path or a `DataReader` becomes a source | [`TileSource`] — the tiles, object‑safe, wrappable by adapters |
//! | **Write** | [`TilesWriter`] — serialise a whole source in one pass | [`TileSink`] — take tiles one at a time, shared across threads |
//!
//! The read row is a pipeline: a [`TilesReader`] *opens* something and hands
//! back a [`TileSource`], and everything downstream — adapters, the server, the
//! VPL pipeline — only ever sees the source.
//!
//! The write row is a choice, and it is the one worth understanding. A
//! [`TilesWriter`] is handed the entire source and writes the file its own way,
//! which is what `versatiles convert` uses. A [`TileSink`] is the opposite: it
//! is wrapped in an `Arc`, shared by several threads, and fed tiles in whatever
//! order they finish — which is what `versatiles mosaic` needs, because it
//! composites tiles as they are produced.
//!
//! ## Which formats do what
//!
//! | Format        | Read | Write whole | Write incrementally |
//! | ------------- | :--: | :---------: | :-----------------: |
//! | `.versatiles` |  ✓   |      ✓      |          ✓          |
//! | `.mbtiles`    |  ✓   |      ✓      |          ✓          |
//! | `.tar`        |  ✓   |      ✓      |          ✓          |
//! | directory     |  ✓   |      ✓      |          ✓          |
//! | `.pmtiles`    |  ✓   |      ✓      |          —          |
//!
//! PMTiles is the one gap, and it follows from the format rather than from an
//! omission: a clustered archive stores its tiles ordered by Hilbert index, so
//! the order cannot be known until every tile is in. [`PMTilesWriter`] gets there
//! by reordering through a temporary file, which a sink — receiving tiles as
//! they arrive, with no idea what is still coming — cannot do. Writing PMTiles
//! therefore goes through the registry, not through [`open_tile_sink`].
//!
//! # Features
//! - `cli`: enables human‑readable probing of containers and tiles.
//! - `test`: helpers for integration tests in downstream crates.
//!
//! ## See also
//! - [`ContainerRegistry`]: register custom reader/writer implementations at runtime
//! - [`TileSource`], [`TilesWriter`]: object‑safe traits for IO
//! - [`open_tile_sink`]: pick a [`TileSink`] from a destination path
//! - [`TilesConvertReader`], [`convert_tiles_container`]: convenience conversion helpers

pub mod cache;
/// Re‑exports in‑memory caches and helpers used by readers/writers.
pub use cache::*;

mod container;
/// Re‑exports the container registry and common open/write helpers.
pub use container::*;

pub mod probe;
pub use probe::*;

pub mod progress;
pub use progress::*;

/// Re‑exports progress tracking and event bus types.
pub mod runtime;
pub use runtime::*;

mod traversal;
pub use traversal::*;

mod types;
/// Re‑exports reader/writer traits, converters, and auxiliary types.
pub use types::*;

#[cfg(any(test, feature = "test"))]
pub mod testing;
