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

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;

	/// Test that ProgressData can be created and has expected fields
	#[test]
	fn test_progress_data_export() {
		let data = ProgressData {
			position: 100.0,
			total: 1000.0,
			percentage: 10.0,
			speed: 50.0,
			estimated_seconds_remaining: Some(180.0),
			eta: Some(1234567890.0),
			message: Some("Processing".to_string()),
		};
		assert_eq!(data.position, 100.0);
		assert_eq!(data.total, 1000.0);
		assert_eq!(data.percentage, 10.0);
		assert_eq!(data.speed, 50.0);
		assert_eq!(data.estimated_seconds_remaining, Some(180.0));
		assert_eq!(data.eta, Some(1234567890.0));
		assert_eq!(data.message, Some("Processing".to_string()));
	}

	/// Test that ConvertOptions can be created with defaults
	#[test]
	fn test_convert_options_export() {
		let opts = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};
		assert!(opts.min_zoom.is_none());
	}

	/// Test that ServerOptions can be created
	#[test]
	fn test_server_options_export() {
		let opts = ServerOptions {
			port: Some(8080),
			ip: Some("127.0.0.1".to_string()),
			minimal_recompression: None,
		};
		assert_eq!(opts.port, Some(8080));
		assert_eq!(opts.ip, Some("127.0.0.1".to_string()));
	}

	/// Test that TileCoord can be created via constructor
	#[test]
	fn test_tile_coord_export() {
		let coord = TileCoord::new(10, 512, 384).unwrap();
		assert_eq!(coord.z(), 10);
		assert_eq!(coord.x(), 512);
		assert_eq!(coord.y(), 384);
	}

	/// Test that SourceMetadata has expected structure
	#[test]
	fn test_source_metadata_export() {
		let metadata = SourceMetadata {
			tile_format: "pbf".to_string(),
			tile_compression: "gzip".to_string(),
			min_zoom: 0,
			max_zoom: 14,
		};
		assert_eq!(metadata.tile_format, "pbf");
		assert_eq!(metadata.tile_compression, "gzip");
		assert_eq!(metadata.min_zoom, 0);
		assert_eq!(metadata.max_zoom, 14);
	}

	/// Test that ProbeResult has expected structure
	#[test]
	fn test_probe_result_export() {
		let metadata = SourceMetadata {
			tile_format: "pbf".to_string(),
			tile_compression: "brotli".to_string(),
			min_zoom: 0,
			max_zoom: 14,
		};
		let result = ProbeResult {
			source_name: "test.versatiles".to_string(),
			container_name: "versatiles".to_string(),
			tile_json: "{}".to_string(),
			parameters: metadata,
		};
		assert_eq!(result.source_name, "test.versatiles");
		assert_eq!(result.container_name, "versatiles");
		assert_eq!(result.tile_json, "{}");
		assert_eq!(result.parameters.tile_format, "pbf");
	}
}
