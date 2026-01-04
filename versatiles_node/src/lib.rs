//! # VersaTiles Node.js Bindings
//!
//! Native Node.js bindings for the VersaTiles library, providing high-performance
//! tile container operations and HTTP tile serving capabilities.
//!
//! ## Features
//!
//! - **Container Reading**: Read tiles from various formats (.versatiles, .mbtiles, .pmtiles, .tar, directories)
//! - **Tile Conversion**: Convert between different tile container formats with progress monitoring
//! - **HTTP Server**: Serve tiles and static content over HTTP with hot reload support
//! - **Progress Monitoring**: Real-time progress updates for long-running operations
//!
//! ## Main Types
//!
//! - [`TileSource`]: Read tiles from various container formats
//! - [`TileServer`]: HTTP server for serving tiles and static content
//! - [`Progress`]: Progress monitoring for conversion operations
//! - [`ConvertOptions`]: Configuration for tile conversion
//!
//! ## Example Usage
//!
//! ```javascript
//! const { TileSource, TileServer, convert } = require('versatiles');
//!
//! // Read tiles from a container
//! const reader = await TileSource.open('tiles.versatiles');
//! const tile = await reader.getTile(10, 512, 384);
//!
//! // Convert tiles with progress monitoring
//! await convert('input.mbtiles', 'output.versatiles', {
//!   minZoom: 0,
//!   maxZoom: 14,
//!   compress: 'brotli'
//! }, (progress) => {
//!   console.log(`${progress.percentage.toFixed(1)}%`);
//! });
//!
//! // Start a tile server
//! const server = new TileServer({ port: 8080 });
//! await server.addTileSource('osm', 'tiles.versatiles');
//! await server.start();
//! ```

#![deny(clippy::all)]

mod convert;
mod macros;
mod progress;
mod runtime;
mod server;
mod tile_source;
mod types;

pub use convert::convert;
pub use progress::{Progress, ProgressData};
pub use server::TileServer;
pub use tile_source::TileSource;
pub use types::{ConvertOptions, ProbeResult, ServerOptions, SourceMetadata, TileCoord};

/// Initialize logging when the module loads.
/// This ensures that log messages from the Rust code are visible in the Node.js console.
///
/// The logger is configured to:
/// - Output to stderr (so it doesn't interfere with stdout)
/// - Use the log level from the `RUST_LOG` environment variable (default: INFO)
/// - Format messages with a simple prefix (ERROR/WARN/INFO/DEBUG/TRACE)
///
/// Environment variable examples:
/// - `RUST_LOG=debug` - Show debug and above
/// - `RUST_LOG=warn` - Show warnings and errors only
/// - `RUST_LOG=versatiles=debug,versatiles_core=trace` - Fine-grained control
#[ctor::ctor]
fn init_logger() {
	use std::io::Write;

	let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
		.format(|buf, record| {
			let level = record.level();
			let prefix = match level {
				log::Level::Error => "ERROR: ",
				log::Level::Warn => "WARN: ",
				log::Level::Info => "info: ",
				log::Level::Debug => "debug: ",
				log::Level::Trace => "trace: ",
			};
			writeln!(buf, "{}{}", prefix, record.args())
		})
		.target(env_logger::Target::Stderr)
		.try_init();
}
