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
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::{SourceType as RustSourceType, TileSourceTrait};
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
pub struct ContainerReader {
	reader: Arc<Mutex<Box<dyn TileSourceTrait>>>,
}

#[napi]
impl ContainerReader {
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
		let reader = napi_result!(runtime.get_reader_from_str(&path).await)?;

		Ok(Self {
			reader: Arc::new(Mutex::new(reader)),
		})
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

	/// Get reader parameters
	///
	/// Returns detailed technical parameters about the tile container including
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
	/// const params = await reader.parameters();
	/// console.log(`Format: ${params.tileFormat}`);
	/// console.log(`Compression: ${params.tileCompression}`);
	/// console.log(`Zoom: ${params.minZoom}-${params.maxZoom}`);
	/// ```
	#[napi]
	pub async fn parameters(&self) -> SourceMetadata {
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
