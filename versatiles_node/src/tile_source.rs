/// Tile container reader
///
/// This module provides the [`TileSource`] class for reading tiles from
/// various container formats. It supports both local files and remote URLs.
///
/// ## Supported Formats
///
/// - **VersaTiles** (.versatiles) - Native format, local and remote
/// - **MBTiles** (.mbtiles) - SQLite-based format, local only
/// - **PMTiles** (.pmtiles) - Cloud-optimized format, local and remote
/// - **TAR** (.tar) - Archive format, local only
/// - **Directories** - Tile directories following standard naming conventions
use crate::{
	convert::convert_tiles_with_options,
	napi_result,
	progress::{MessageData, ProgressData},
	runtime::create_runtime,
	types::{ConvertOptions, SourceMetadata, TileJSON},
};
use napi::{bindgen_prelude::*, threadsafe_function::ThreadsafeFunction};
use napi_derive::napi;
use std::{path::PathBuf, sync::Arc};
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
#[derive(Clone)]
pub struct TileSource {
	reader: Arc<Box<dyn RustTileSource>>,
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

	fn new(source: Arc<Box<dyn RustTileSource>>) -> Self {
		Self { reader: source }
	}

	/// Create a new reader from this TileSource (for server use)
	/// This recreates the reader, which is needed when the server API requires ownership
	pub(crate) fn reader(&self) -> Arc<Box<dyn RustTileSource>> {
		self.reader.clone()
	}

	/// Create a TileSource from a VPL (VersaTiles Pipeline Language) string
	///
	/// VPL allows you to define tile processing pipelines that can filter, transform,
	/// and combine tile sources. This is useful for on-the-fly tile manipulation without
	/// creating intermediate files.
	///
	/// # Arguments
	///
	/// * `vpl` - VPL pipeline definition string
	/// * `dir` - Optional base directory for resolving relative file paths in the VPL.
	///   Defaults to the current working directory if not specified.
	///
	/// # VPL Operations
	///
	/// Common VPL operations include:
	/// - `from_container filename="..."` - Load tiles from a container file
	/// - `filter level_min=X level_max=Y` - Filter by zoom levels
	/// - `filter bbox=[west,south,east,north]` - Filter by geographic bounds
	/// - Pipeline operators can be chained with `|`
	///
	/// # Returns
	///
	/// A `TileSource` instance representing the VPL pipeline
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The VPL syntax is invalid
	/// - Referenced files don't exist or are inaccessible
	/// - Pipeline operations are incompatible
	///
	/// # Examples
	///
	/// ```javascript
	/// // Simple container loading with explicit directory
	/// const source = await TileSource.fromVpl(
	///   'from_container filename="tiles.mbtiles"',
	///   '/path/to/tiles'
	/// );
	///
	/// // Using current directory (omit second argument)
	/// const source2 = await TileSource.fromVpl(
	///   'from_container filename="tiles.mbtiles"'
	/// );
	///
	/// // Filter by zoom level
	/// const filtered = await TileSource.fromVpl(
	///   'from_container filename="tiles.mbtiles" | filter level_min=5 level_max=10',
	///   '/path/to/tiles'
	/// );
	///
	/// // Filter by geographic area
	/// const berlin = await TileSource.fromVpl(
	///   'from_container filename="world.mbtiles" | filter bbox=[13.0,52.0,14.0,53.0]',
	///   '/path/to/tiles'
	/// );
	/// ```
	///
	/// # Note
	///
	/// VPL sources can be converted using `convertTo()` just like any other tile source.
	/// The conversion will process the pipeline and write the output tiles to the target format.
	#[napi(factory)]
	pub async fn from_vpl(vpl: String, dir: Option<String>) -> Result<Self> {
		let runtime = create_runtime();
		let path = if let Some(d) = dir {
			PathBuf::from(&d)
		} else {
			std::env::current_dir()?
		};
		let source = napi_result!(PipelineReader::open_str(&vpl, &path, runtime).await)?;
		Ok(Self::new(Arc::new(Box::new(source))))
	}

	/// Convert this tile source to another format
	///
	/// Converts the current tile source to a different container format with optional
	/// filtering, transformation, and compression changes. Supports real-time progress
	/// monitoring through callback functions.
	///
	/// # Arguments
	///
	/// * `output` - Path to the output tile container
	/// * `options` - Optional conversion options (zoom range, bbox, compression, etc.)
	/// * `on_progress` - Optional callback for progress updates
	/// * `on_message` - Optional callback for step/warning/error messages
	///
	/// # Conversion Options
	///
	/// - `minZoom` / `maxZoom`: Filter to specific zoom levels
	/// - `bbox`: Geographic bounding box `[west, south, east, north]`
	/// - `bboxBorder`: Add border tiles around bbox (in tile units)
	/// - `compress`: Output compression ("gzip", "brotli", "uncompressed")
	/// - `flipY`: Flip tiles vertically (TMS ↔ XYZ coordinate systems)
	/// - `swapXY`: Swap X and Y tile coordinates
	///
	/// # Progress Callbacks
	///
	/// **onProgress callback** receives:
	/// - `position`: Current tile count
	/// - `total`: Total tile count
	/// - `percentage`: Progress percentage (0-100)
	/// - `speed`: Processing speed (tiles/second)
	/// - `eta`: Estimated completion time (as JavaScript Date)
	///
	/// **onMessage callback** receives:
	/// - `type`: Message type ("step", "warning", or "error")
	/// - `message`: The message text
	///
	/// # Returns
	///
	/// A Promise that resolves when conversion is complete
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - Output path is invalid or not writable
	/// - Bbox coordinates are invalid (must be `[west, south, east, north]`)
	/// - Compression format is not recognized
	/// - An I/O error occurs during conversion
	///
	/// # Examples
	///
	/// ```javascript
	/// const source = await TileSource.open('input.mbtiles');
	///
	/// // Simple conversion
	/// await source.convertTo('output.versatiles');
	///
	/// // Convert with compression
	/// await source.convertTo('output.versatiles', {
	///   compress: 'brotli'
	/// });
	///
	/// // Convert specific area and zoom range
	/// await source.convertTo('europe.versatiles', {
	///   minZoom: 0,
	///   maxZoom: 14,
	///   bbox: [-10, 35, 40, 70], // Europe
	///   bboxBorder: 1
	/// });
	///
	/// // With progress monitoring
	/// await source.convertTo('output.versatiles', null,
	///   (progress) => {
	///     console.log(`${progress.percentage.toFixed(1)}% complete`);
	///   },
	///   (type, message) => {
	///     if (type === 'error') console.error(message);
	///   }
	/// );
	/// ```
	#[napi]
	pub async fn convert_to(
		&self,
		output: String,
		options: Option<ConvertOptions>,
		on_progress: Option<ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false, true>>,
		on_message: Option<ThreadsafeFunction<MessageData, Unknown<'static>, MessageData, Status, false, true>>,
	) -> Result<()> {
		// Use shared conversion logic
		let output_path = std::path::PathBuf::from(&output);
		convert_tiles_with_options(self.reader.clone(), &output_path, options, on_progress, on_message).await
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
		if z > 30 {
			return Err(Error::from_reason("Zoom level must be between 0 and 30"));
		}
		#[allow(clippy::cast_possible_truncation)]
		let coord = napi_result!(RustTileCoord::new(z as u8, x, y))?;
		let tile_opt = napi_result!(self.reader.get_tile(&coord).await)?;

		tile_opt
			.map(|mut tile| {
				tile
					.as_blob(versatiles_core::TileCompression::Uncompressed)
					.map(|blob| Buffer::from(blob.as_slice()))
			})
			.transpose()
			.map_err(|e| Error::from_reason(format!("Failed to decompress tile: {e}")))
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
	pub fn tile_json(&self) -> TileJSON {
		TileJSON::build(self.reader.tilejson(), &self.reader.metadata().bbox_pyramid)
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
	pub fn metadata(&self) -> SourceMetadata {
		SourceMetadata::from(self.reader.metadata())
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
	pub fn source_type(&self) -> SourceType {
		self.reader.source_type().as_ref().into()
	}
}

/// Information about the tile source type and origin
///
/// Provides metadata about how the tile source was created and what it represents.
/// This is useful for debugging and understanding the source of tiles being served.
///
/// # Source Kinds
///
/// - **Container**: A file-based tile container (MBTiles, PMTiles, VersaTiles, TAR, or directory)
/// - **Processor**: A tile source with transformations applied (e.g., from VPL pipelines)
/// - **Composite**: Multiple tile sources combined together
///
/// # Properties
///
/// All source types have:
/// - `kind`: The type of source ("container", "processor", or "composite")
/// - `name`: A descriptive name for the source
///
/// Container sources also have:
/// - `uri`: The file path or URL of the container
///
/// Processor sources also have:
/// - `input`: The source being processed
///
/// Composite sources also have:
/// - `inputs`: Array of combined sources

#[napi]
pub struct SourceType {
	// Internal storage - not exposed to JS
	inner: Arc<RustSourceType>,
}

#[napi]
impl SourceType {
	/// Get the kind of source
	///
	/// Returns one of:
	/// - `"container"`: File-based tile container
	/// - `"processor"`: Transformed tile source (e.g., VPL pipeline)
	/// - `"composite"`: Multiple sources combined
	#[napi(getter)]
	pub fn kind(&self) -> String {
		match self.inner.as_ref() {
			RustSourceType::Container { .. } => "container".to_string(),
			RustSourceType::Processor { .. } => "processor".to_string(),
			RustSourceType::Composite { .. } => "composite".to_string(),
		}
	}

	/// Get the name of the source
	///
	/// Returns a descriptive name indicating the source format or type.
	/// For containers, this is typically the format name (e.g., "MBTiles", "PMTiles").
	/// For processors, this describes the transformation (e.g., "filter", "transform").
	#[napi(getter)]
	pub fn name(&self) -> String {
		match self.inner.as_ref() {
			RustSourceType::Container { name, .. }
			| RustSourceType::Processor { name, .. }
			| RustSourceType::Composite { name, .. } => name.clone(),
		}
	}

	/// Get the file path or URL (for Container sources only)
	///
	/// Returns the full path or URL of the tile container file.
	/// Returns `null` for Processor and Composite sources.
	///
	/// # Example
	///
	/// ```javascript
	/// const source = await TileSource.open('tiles.mbtiles');
	/// const type = source.sourceType();
	/// console.log(type.uri);  // "/path/to/tiles.mbtiles"
	/// ```
	#[napi(getter)]
	pub fn uri(&self) -> Option<String> {
		match self.inner.as_ref() {
			RustSourceType::Container { input, .. } => Some(input.clone()),
			_ => None,
		}
	}

	/// Get the input source being processed (for Processor sources only)
	///
	/// Returns the source that this processor is transforming.
	/// Returns `null` for Container and Composite sources.
	///
	/// # Example
	///
	/// ```javascript
	/// const source = await TileSource.fromVpl(
	///   'from_container filename="tiles.mbtiles" | filter level_min=5',
	///   '.'
	/// );
	/// const type = source.sourceType();
	/// console.log(type.kind);  // "processor"
	/// console.log(type.name);  // "filter"
	/// const inputType = type.input;
	/// console.log(inputType.kind);  // "container"
	/// ```
	#[napi(getter)]
	pub fn input(&self) -> Option<SourceType> {
		match self.inner.as_ref() {
			RustSourceType::Processor { input, .. } => Some(SourceType {
				inner: Arc::clone(input),
			}),
			_ => None,
		}
	}

	/// Get the array of combined sources (for Composite sources only)
	///
	/// Returns an array of all sources that have been combined together.
	/// Returns `null` for Container and Processor sources.
	///
	/// # Example
	///
	/// ```javascript
	/// // For a composite source combining multiple tile sets
	/// const type = source.sourceType();
	/// if (type.kind === 'composite') {
	///   const sources = type.inputs;
	///   console.log(`Combined ${sources.length} tile sources`);
	/// }
	/// ```
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
	async fn test_tile_json_mbtiles() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let tile_json = reader.tile_json();

		// MBTiles should have bounds
		assert_eq!(tile_json.bounds.unwrap(), [13.08283, 52.33446, 13.762245, 52.6783]);

		// Check common fields
		assert_eq!(tile_json.tilejson, "3.0");
		assert_eq!(tile_json.minzoom, 0.0);
		assert_eq!(tile_json.maxzoom, 14.0);
		assert!(tile_json.vector_layers.is_some());
		assert_eq!(tile_json.vector_layers.as_ref().unwrap().len(), 19);
	}

	#[tokio::test]
	async fn test_tile_json_pmtiles() {
		let reader = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		let tile_json = reader.tile_json();

		// PMTiles doesn't have bounds in metadata
		assert!(tile_json.bounds.is_none());

		// Check common fields
		assert_eq!(tile_json.tilejson, "3.0");
		assert_eq!(tile_json.minzoom, 0.0);
		assert_eq!(tile_json.maxzoom, 14.0);
		assert!(tile_json.vector_layers.is_some());
		assert_eq!(tile_json.vector_layers.as_ref().unwrap().len(), 19);
	}

	#[tokio::test]
	async fn test_metadata_valid() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata();

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

		let metadata = reader.metadata();

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

		let metadata = reader.metadata();

		// Berlin test data should be in a known format
		let valid_formats = ["png", "jpg", "jpeg", "webp", "pbf", "mvt"];
		assert!(valid_formats.contains(&metadata.tile_format.as_str()));
	}

	#[tokio::test]
	async fn test_metadata_compression() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let metadata = reader.metadata();

		// Should have a valid compression type
		let valid_compressions = ["uncompressed", "gzip", "brotli", "zstd"];
		assert!(valid_compressions.contains(&metadata.tile_compression.as_str()));
	}

	#[tokio::test]
	async fn test_source_type_container() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type();

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

		let source_type = reader.source_type();

		// Name should indicate the format
		let name = source_type.name();
		assert!(name.contains("mbtiles") || name.contains("MBTiles"));
	}

	#[tokio::test]
	async fn test_source_type_uri_mbtiles() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type();

		// URI should contain the path
		let uri = source_type.uri().unwrap();
		assert!(uri.contains("berlin.mbtiles"));
	}

	#[tokio::test]
	async fn test_source_type_uri_pmtiles() {
		let reader = TileSource::open("../testdata/berlin.pmtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type();

		let uri = source_type.uri().unwrap();
		assert!(uri.contains("berlin.pmtiles"));
	}

	#[tokio::test]
	async fn test_source_type_container_no_input() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type();

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
		let metadata1 = reader1.metadata();
		let metadata2 = reader2.metadata();

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
	async fn test_source_type_kind_values() {
		let reader = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		let source_type = reader.source_type();
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
		let metadata1 = reader.metadata();
		let metadata2 = reader.metadata();

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
		let json1 = reader.tile_json();
		let json2 = reader.tile_json();

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

	#[tokio::test]
	async fn test_from_vpl_simple_container() {
		// Test loading a simple VPL that just references a container
		let vpl = r#"from_container filename="berlin.mbtiles""#;
		let reader = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		// Verify we can read metadata
		let metadata = reader.metadata();
		assert_eq!(metadata.tile_format, "mvt");
		assert_eq!(metadata.tile_compression, "gzip");
		assert_eq!(metadata.min_zoom, 0);
		assert_eq!(metadata.max_zoom, 14);
	}

	#[tokio::test]
	async fn test_from_vpl_file() {
		// Test loading a VPL file that includes transformations
		let vpl = std::fs::read_to_string("../testdata/berlin.vpl").unwrap();
		let reader = TileSource::from_vpl(vpl, Some("../testdata".to_string()))
			.await
			.unwrap();

		// Verify we can read metadata
		let metadata = reader.metadata();
		assert_eq!(metadata.tile_format, "mvt");
		assert_eq!(metadata.tile_compression, "gzip");

		// Verify we can get tiles
		let tile = reader.get_tile(10, 550, 335).await.unwrap();
		assert!(tile.is_some());
	}

	#[tokio::test]
	async fn test_from_vpl_with_pipeline() {
		// Test a VPL with multiple pipeline operations
		let vpl = r#"from_container filename="berlin.mbtiles" | filter level_min=5 level_max=10"#;
		let reader = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		// Verify metadata reflects the zoom filter
		let metadata = reader.metadata();
		assert_eq!(metadata.min_zoom, 5);
		assert_eq!(metadata.max_zoom, 10);
	}

	#[tokio::test]
	async fn test_from_vpl_tile_retrieval() {
		// Test that we can retrieve tiles through VPL
		let vpl = r#"from_container filename="berlin.mbtiles""#;
		let reader = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		// Get a tile that should exist
		let tile = reader.get_tile(5, 17, 10).await.unwrap();
		assert_eq!(tile.unwrap().len(), 4137);
	}

	#[tokio::test]
	async fn test_from_vpl_tilejson() {
		// Test that TileJSON works with VPL sources
		let vpl = r#"from_container filename="berlin.mbtiles""#;
		let reader = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		let tile_json = reader.tile_json();
		assert_eq!(tile_json.tilejson, "3.0");
		assert_eq!(tile_json.minzoom, 0.0);
		assert_eq!(tile_json.maxzoom, 14.0);
		assert!(tile_json.vector_layers.is_some());
	}

	#[tokio::test]
	async fn test_from_vpl_invalid_syntax() {
		// Test that invalid VPL returns an error
		let vpl = r"invalid vpl syntax here";
		let result = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string())).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_from_vpl_nonexistent_file() {
		// Test that referencing a non-existent file returns an error
		let vpl = r#"from_container filename="nonexistent.mbtiles""#;
		let result = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string())).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_from_vpl_source_type() {
		// Test that source_type works correctly for VPL sources
		let vpl = r#"from_container filename="berlin.mbtiles""#;
		let reader = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		let source_type = reader.source_type();
		assert_eq!(source_type.kind(), "processor");
		assert!(source_type.input().is_some());
	}

	#[tokio::test]
	async fn test_convert_to() {
		// Create a temp output file path
		let output_path = std::env::temp_dir().join("test_convert_to.versatiles");

		// Open a tile source
		let source = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Convert to versatiles format
		let result = source
			.convert_to(output_path.to_str().unwrap().to_string(), None, None, None)
			.await;

		// Should succeed
		assert!(result.is_ok(), "Conversion should succeed");

		// Verify output file exists and has content
		assert!(output_path.exists(), "Output file should exist");
		let metadata = std::fs::metadata(&output_path).unwrap();
		assert!(metadata.len() > 0, "Output file should not be empty");

		// Clean up
		let _ = std::fs::remove_file(&output_path);
	}

	#[tokio::test]
	async fn test_convert_to_with_options() {
		// Create a temp output file path
		let output_path = std::env::temp_dir().join("test_convert_to_with_options.versatiles");

		// Open a tile source
		let source = TileSource::open("../testdata/berlin.mbtiles".to_string())
			.await
			.unwrap();

		// Convert with options (zoom filter)
		let options = ConvertOptions {
			min_zoom: Some(5),
			max_zoom: Some(10),
			bbox: None,
			bbox_border: None,
			compress: Some("gzip".to_string()),
			flip_y: None,
			swap_xy: None,
		};

		let result = source
			.convert_to(output_path.to_str().unwrap().to_string(), Some(options), None, None)
			.await;

		// Should succeed
		assert!(result.is_ok(), "Conversion with options should succeed");

		// Verify output file exists
		assert!(output_path.exists(), "Output file should exist");

		// Clean up
		let _ = std::fs::remove_file(&output_path);
	}

	#[tokio::test]
	async fn test_convert_to_vpl_source() {
		// Create a temp output file path
		let output_path = std::env::temp_dir().join("test_convert_to_vpl.versatiles");

		// Create a VPL source
		let vpl = r#"from_container filename="berlin.mbtiles""#;
		let source = TileSource::from_vpl(vpl.to_string(), Some("../testdata".to_string()))
			.await
			.unwrap();

		// Try to convert
		source
			.convert_to(output_path.to_str().unwrap().to_string(), None, None, None)
			.await
			.unwrap();

		// Clean up
		let _ = std::fs::remove_file(&output_path);
	}
}
