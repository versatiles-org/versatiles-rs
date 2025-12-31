/// Tile container reader
///
/// This module provides the [`ContainerReader`] class for reading tiles from
/// various container formats. It supports both local files and remote URLs.
///
/// ## Supported Formats
///
/// - **VersaTiles** (.versatiles) - Native format, local and remote
/// - **MBTiles** (.mbtiles) - SQLite-based format, local only
/// - **PMTiles** (.pmtiles) - Cloud-optimized format, local and remote
/// - **TAR** (.tar) - Archive format, local only
/// - **Directories** - Tile directories following standard naming conventions
use crate::{napi_result, runtime::create_runtime, types::SourceMetadata};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use versatiles::pipeline::PipelineReader;
use versatiles_container::{SourceType as RustSourceType, TileSource as RustTileSource};
use versatiles_core::TileCoord as RustTileCoord;

/// Container reader for accessing tile data from various formats
///
/// Provides async methods to read tiles and metadata from supported container formats.
/// All operations are thread-safe and can be called concurrently from multiple tasks.
///
/// # Supported Formats
///
/// **Local files:**
/// - `.versatiles` - VersaTiles native format
/// - `.mbtiles` - MBTiles (SQLite-based)
/// - `.pmtiles` - PMTiles (cloud-optimized)
/// - `.tar` - TAR archives
/// - Directories - Standard tile directory structure
///
/// **Remote URLs:**
/// - `.versatiles` files via HTTP/HTTPS
/// - `.pmtiles` files via HTTP/HTTPS (with range request support)
#[napi]
pub struct TileSource {
	reader: Arc<Mutex<Box<dyn RustTileSource>>>,
}

#[napi]
impl TileSource {
	/// Open a tile container from a file path or URL
	///
	/// Automatically detects the container format based on the file extension
	/// or content. Supports both local files and remote URLs.
	///
	/// # Arguments
	///
	/// * `path` - File path or URL to the tile container
	///
	/// # Returns
	///
	/// A `ContainerReader` instance ready to read tiles
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The file or URL doesn't exist or is inaccessible
	/// - The format is not recognized or supported
	/// - The file is corrupted or invalid
	///
	/// # Examples
	///
	/// ```javascript
	/// // Open local file
	/// const reader = await ContainerReader.open('tiles.versatiles');
	///
	/// // Open remote file
	/// const reader = await ContainerReader.open('https://example.com/tiles.pmtiles');
	/// ```
	#[napi(factory)]
	pub async fn open(path: String) -> Result<Self> {
		let runtime = create_runtime();
		let source = napi_result!(runtime.get_reader_from_str(&path).await)?;
		Ok(Self::new(source))
	}

	fn new(source: Box<dyn RustTileSource>) -> Self {
		Self {
			reader: Arc::new(Mutex::new(source)),
		}
	}

	/// Get a ContainerReader instance from an VPL string
	#[napi(factory)]
	pub async fn from_vpl(vpl: String, dir: String) -> Result<Self> {
		let runtime = create_runtime();
		let source = napi_result!(PipelineReader::open_str(&vpl, Path::new(&dir), runtime).await)?;
		Ok(Self::new(Box::new(source)))
	}

	/// Get a single tile at the specified coordinates
	///
	/// Retrieves the tile data for the given zoom level (z) and tile coordinates (x, y).
	/// The tile is automatically decompressed and returned as a Buffer.
	///
	/// # Arguments
	///
	/// * `z` - Zoom level (0-32)
	/// * `x` - Tile column (0 to 2^z - 1)
	/// * `y` - Tile row (0 to 2^z - 1)
	///
	/// # Returns
	///
	/// - `Some(Buffer)` containing the uncompressed tile data if the tile exists
	/// - `None` if the tile doesn't exist at these coordinates
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The coordinates are invalid (out of bounds for the zoom level)
	/// - An I/O error occurs while reading the tile
	///
	/// # Examples
	///
	/// ```javascript
	/// const tile = await reader.getTile(10, 512, 384);
	/// if (tile) {
	///   console.log(`Tile size: ${tile.length} bytes`);
	/// }
	/// ```
	#[napi]
	pub async fn get_tile(&self, z: u32, x: u32, y: u32) -> Result<Option<Buffer>> {
		let coord = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		let reader = self.reader.lock().await;
		let tile_opt = napi_result!(reader.get_tile(&coord).await)?;

		Ok(tile_opt.map(|mut tile| {
			let blob = tile.as_blob(versatiles_core::TileCompression::Uncompressed).unwrap();
			Buffer::from(blob.as_slice())
		}))
	}

	/// Get TileJSON metadata as a JSON string
	///
	/// Returns the container's metadata in TileJSON format, which includes
	/// information about tile bounds, zoom levels, attribution, and more.
	///
	/// # Returns
	///
	/// A JSON string containing TileJSON metadata
	///
	/// # Examples
	///
	/// ```javascript
	/// const tileJson = await reader.tileJson();
	/// const metadata = JSON.parse(tileJson);
	/// console.log(`Zoom range: ${metadata.minzoom} - ${metadata.maxzoom}`);
	/// ```
	#[napi]
	pub async fn tile_json(&self) -> String {
		let reader = self.reader.lock().await;
		reader.tilejson().as_string()
	}

	/// Get reader metadata
	///
	/// Returns detailed technical metadata about the tile container including
	/// format, compression, and available zoom levels.
	///
	/// # Returns
	///
	/// A [`SourceMetadata`] object containing:
	/// - `tileFormat`: The tile format (e.g., "png", "jpg", "mvt")
	/// - `tileCompression`: The compression method (e.g., "gzip", "brotli", "uncompressed")
	/// - `minZoom`: Minimum available zoom level
	/// - `maxZoom`: Maximum available zoom level
	///
	/// # Examples
	///
	/// ```javascript
	/// const metadata = await reader.metadata();
	/// console.log(`Format: ${metadata.tileFormat}`);
	/// console.log(`Compression: ${metadata.tileCompression}`);
	/// console.log(`Zoom: ${metadata.minZoom}-${metadata.maxZoom}`);
	/// ```
	#[napi]
	pub async fn metadata(&self) -> SourceMetadata {
		let reader = self.reader.lock().await;
		SourceMetadata::from(reader.metadata())
	}

	/// Get the source type
	///
	/// Returns the name or path of the data source being read.
	///
	/// # Returns
	///
	/// The source type (typically the file path or URL)
	///
	/// # Examples
	///
	/// ```javascript
	/// const sourceType = await reader.sourceType();
	/// console.log(sourceType.kind);  // "container", "processor", or "composite"
	/// console.log(sourceType.name);  // e.g., "mbtiles"
	/// if (sourceType.kind === "container") {
	///   console.log(sourceType.uri);  // file path or URL
	/// }
	/// ```
	#[napi]
	pub async fn source_type(&self) -> SourceType {
		self.reader.lock().await.source_type().as_ref().into()
	}
}

/// SourceType exposed to JavaScript as a class with getters and methods.
///
/// Properties (getters):
/// - kind: "container" | "processor" | "composite"
/// - name: string
/// - uri: string | null (only for Container)
///
/// Methods:
/// - input(): SourceType | null (only for Processor)
/// - inputs(): SourceType[] | null (only for Composite)

#[napi]
pub struct SourceType {
	// Internal storage - not exposed to JS
	inner: Arc<RustSourceType>,
}

#[napi]
impl SourceType {
	/// Get the kind of source ("container", "processor", or "composite")
	#[napi(getter)]
	pub fn kind(&self) -> String {
		match self.inner.as_ref() {
			RustSourceType::Container { .. } => "container".to_string(),
			RustSourceType::Processor { .. } => "processor".to_string(),
			RustSourceType::Composite { .. } => "composite".to_string(),
		}
	}

	/// Get the name of the source
	#[napi(getter)]
	pub fn name(&self) -> String {
		match self.inner.as_ref() {
			RustSourceType::Container { name, .. } => name.clone(),
			RustSourceType::Processor { name, .. } => name.clone(),
			RustSourceType::Composite { name, .. } => name.clone(),
		}
	}

	/// Get the URI (for Container type only)
	#[napi(getter)]
	pub fn uri(&self) -> Option<String> {
		match self.inner.as_ref() {
			RustSourceType::Container { input, .. } => Some(input.clone()),
			_ => None,
		}
	}

	/// Get the input source (for Processor type only)
	#[napi(getter)]
	pub fn input(&self) -> Option<SourceType> {
		match self.inner.as_ref() {
			RustSourceType::Processor { input, .. } => Some(SourceType {
				inner: Arc::clone(input),
			}),
			_ => None,
		}
	}

	/// Get the input sources (for Composite type only)
	#[napi(getter)]
	pub fn inputs(&self) -> Option<Vec<SourceType>> {
		match self.inner.as_ref() {
			RustSourceType::Composite { inputs, .. } => Some(
				inputs
					.iter()
					.map(|input| SourceType {
						inner: Arc::clone(input),
					})
					.collect(),
			),
			_ => None,
		}
	}
}

impl From<&RustSourceType> for SourceType {
	fn from(src: &RustSourceType) -> Self {
		SourceType {
			inner: Arc::new(src.clone()),
		}
	}
}

impl From<Arc<RustSourceType>> for SourceType {
	fn from(inner: Arc<RustSourceType>) -> Self {
		SourceType { inner }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::json::parse_json_str;

	#[tokio::test]
	async fn test_open_valid_mbtiles() {
		let result = TileSource::open("../testdata/berlin.mbtiles".to_string()).await;
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_open_valid_pmtiles() {
		let result = TileSource::open("../testdata/berlin.pmtiles".to_string()).await;
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_open_invalid_path() {
		let result = TileSource::open("/nonexistent/file.mbtiles".to_string()).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_open_invalid_extension() {
		// Create a temporary file with unsupported extension
		let result = TileSource::open("/tmp/invalid.xyz".to_string()).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_tile_valid() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Berlin test data typically has tiles at zoom level 10
		// Using coordinates that should exist
		let tile = reader.get_tile(10, 550, 335).await.unwrap();

		// Tile might or might not exist, but the call should succeed
		if let Some(buffer) = tile {
			// If tile exists, verify it's a valid buffer
			assert!(!buffer.is_empty());
		}
	}

	#[tokio::test]
	async fn test_get_tile_non_existent() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Request a tile at coordinates that definitely don't exist (zoom 0, extreme coordinates)
		let tile = reader.get_tile(0, 1000, 1000).await;

		// Should return error due to invalid coordinates
		assert!(tile.is_err());
	}

	#[tokio::test]
	async fn test_get_tile_out_of_bounds() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// At zoom 1, max x and y is 1, so (2, 2) is out of bounds
		let result = reader.get_tile(1, 2, 2).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_tile_zero_coords() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Zoom 0, tile (0, 0) is always valid
		let result = reader.get_tile(0, 0, 0).await;
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_tile_json_valid() -> anyhow::Result<()> {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let tile_json = reader.tile_json().await;

		// Verify it's a non-empty string
		assert!(!tile_json.is_empty());

		// Verify it's valid JSON
		let json = parse_json_str(&tile_json)?.into_object()?;

		// Verify it has expected TileJSON fields
		assert_eq!(json.get_string("tilejson")?.unwrap(), "3.0.0");
		assert_eq!(json.get_number("minzoom")?.unwrap(), 0.0);
		assert_eq!(json.get_number("maxzoom")?.unwrap(), 14.0);

		Ok(())
	}

	#[tokio::test]
	async fn test_tile_json_mbtiles_vs_pmtiles() {
		let reader_mbtiles = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();
		let reader_pmtiles = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		let json_mbtiles = reader_mbtiles.tile_json().await;
		let json_pmtiles = reader_pmtiles.tile_json().await;

		// Both should return valid JSON
		assert!(!json_mbtiles.is_empty());
		assert!(!json_pmtiles.is_empty());
	}

	#[tokio::test]
	async fn test_metadata_valid() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata().await;

		// Verify metadata has expected fields
		assert!(!metadata.tile_format.is_empty());
		assert!(!metadata.tile_compression.is_empty());
		assert!(metadata.min_zoom <= metadata.max_zoom);
	}

	#[tokio::test]
	async fn test_metadata_zoom_levels() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata().await;

		// Zoom levels should be valid (0-32)
		assert!(metadata.min_zoom <= 32);
		assert!(metadata.max_zoom <= 32);
		assert!(metadata.min_zoom <= metadata.max_zoom);
	}

	#[tokio::test]
	async fn test_metadata_tile_format() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata().await;

		// Berlin test data should be in a known format
		let valid_formats = ["png", "jpg", "jpeg", "webp", "pbf", "mvt"];
		assert!(valid_formats.contains(&metadata.tile_format.as_str()));
	}

	#[tokio::test]
	async fn test_metadata_compression() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata().await;

		// Should have a valid compression type
		let valid_compressions = ["uncompressed", "gzip", "brotli", "zstd"];
		assert!(valid_compressions.contains(&metadata.tile_compression.as_str()));
	}

	#[tokio::test]
	async fn test_source_type_container() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;

		// Should be a container type
		assert_eq!(source_type.kind(), "container");
		assert!(!source_type.name().is_empty());
		assert!(source_type.uri().is_some());
	}

	#[tokio::test]
	async fn test_source_type_name() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;

		// Name should indicate the format
		let name = source_type.name();
		assert!(name.contains("mbtiles") || name.contains("MBTiles"));
	}

	#[tokio::test]
	async fn test_source_type_uri_mbtiles() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;

		// URI should contain the path
		let uri = source_type.uri().unwrap();
		assert!(uri.contains("berlin.mbtiles"));
	}

	#[tokio::test]
	async fn test_source_type_uri_pmtiles() {
		let reader = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;

		let uri = source_type.uri().unwrap();
		assert!(uri.contains("berlin.pmtiles"));
	}

	#[tokio::test]
	async fn test_source_type_container_no_input() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;

		// Container type should not have input() or inputs()
		assert!(source_type.input().is_none());
		assert!(source_type.inputs().is_none());
	}

	#[tokio::test]
	async fn test_multiple_readers_independent() {
		let reader1 = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();
		let reader2 = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		// Both should work independently
		let metadata1 = reader1.metadata().await;
		let metadata2 = reader2.metadata().await;

		assert!(!metadata1.tile_format.is_empty());
		assert!(!metadata2.tile_format.is_empty());
	}

	#[tokio::test]
	async fn test_concurrent_tile_reads() {
		let reader = Arc::new(
			TileSource::open("../testdata/berlin.mbtiles".to_string())
				.await
				.unwrap(),
		);

		// Spawn multiple concurrent reads
		let reader1 = Arc::clone(&reader);
		let reader2 = Arc::clone(&reader);

		let handle1 = tokio::spawn(async move { reader1.get_tile(10, 550, 335).await });
		let handle2 = tokio::spawn(async move { reader2.get_tile(10, 551, 335).await });

		// Both should complete without error
		let result1 = handle1.await.unwrap();
		let result2 = handle2.await.unwrap();

		assert!(result1.is_ok());
		assert!(result2.is_ok());
	}

	#[tokio::test]
	async fn test_tile_json_is_valid_json() -> anyhow::Result<()> {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let tile_json = reader.tile_json().await;

		// Parse and verify structure
		let json = parse_json_str(&tile_json)?.into_object()?;

		// Check for required TileJSON fields
		assert_eq!(json.get_string("tilejson")?.unwrap(), "3.0.0");

		// Check optional but common fields
		assert_eq!(json.get_number("minzoom")?.unwrap(), 0.0);
		assert_eq!(json.get_number("maxzoom")?.unwrap(), 14.0);

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type_kind_values() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type().await;
		let kind = source_type.kind();

		// Kind should be one of the valid values
		assert!(kind == "container" || kind == "processor" || kind == "composite");
	}

	#[tokio::test]
	async fn test_metadata_consistency_across_calls() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Call metadata multiple times
		let metadata1 = reader.metadata().await;
		let metadata2 = reader.metadata().await;

		// Should return consistent results
		assert_eq!(metadata1.tile_format, metadata2.tile_format);
		assert_eq!(metadata1.tile_compression, metadata2.tile_compression);
		assert_eq!(metadata1.min_zoom, metadata2.min_zoom);
		assert_eq!(metadata1.max_zoom, metadata2.max_zoom);
	}

	#[tokio::test]
	async fn test_tile_json_consistency_across_calls() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Call tile_json multiple times
		let json1 = reader.tile_json().await;
		let json2 = reader.tile_json().await;

		// Should return identical results
		assert_eq!(json1, json2);
	}

	#[tokio::test]
	async fn test_get_tile_returns_buffer() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Get a tile that likely exists
		let buffer = reader.get_tile(10, 550, 335).await.unwrap().unwrap();
		assert_eq!(buffer.len(), 113612);
	}
}
