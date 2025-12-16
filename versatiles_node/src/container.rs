use crate::{
	napi_result,
	progress::Progress,
	progress_callback::ProgressCallback,
	types::{ConvertOptions, ProbeResult, ReaderParameters, parse_compression},
};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::{ContainerRegistry, ProcessingConfig, TilesConverterParameters, TilesReaderTrait};
use versatiles_core::{GeoBBox, TileBBoxPyramid, TileCoord as RustTileCoord};

/// Container reader for accessing tile data from various formats
#[napi]
pub struct ContainerReader {
	reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
	registry: ContainerRegistry,
}

#[napi]
impl ContainerReader {
	/// Open a tile container from a file path or URL
	///
	/// Supports: .versatiles, .mbtiles, .pmtiles, .tar, directories, HTTP URLs
	#[napi(factory)]
	pub async fn open(path: String) -> Result<Self> {
		let registry = ContainerRegistry::default();
		let reader = napi_result!(registry.get_reader_from_str(&path).await)?;

		Ok(Self {
			reader: Arc::new(Mutex::new(reader)),
			registry,
		})
	}

	/// Get a single tile at the specified coordinates
	///
	/// Returns null if the tile doesn't exist
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
	#[napi(getter)]
	pub async fn tile_json(&self) -> String {
		let reader = self.reader.lock().await;
		reader.tilejson().as_string()
	}

	/// Get reader parameters (format, compression, zoom levels)
	#[napi(getter)]
	pub async fn parameters(&self) -> ReaderParameters {
		let reader = self.reader.lock().await;
		ReaderParameters::from(reader.parameters())
	}

	/// Get the source name
	#[napi(getter)]
	pub async fn source_name(&self) -> String {
		let reader = self.reader.lock().await;
		reader.source_name().to_string()
	}

	/// Get the container type name
	#[napi(getter)]
	pub async fn container_name(&self) -> String {
		let reader = self.reader.lock().await;
		reader.container_name().to_string()
	}

	/// Convert this container to another format with progress monitoring
	///
	/// Returns a Progress object that emits events during the conversion.
	/// Use progress.onProgress(callback) to monitor progress updates.
	/// Use progress.onMessage(callback) to receive step/warning/error messages.
	/// Use progress.onComplete(callback) to be notified when complete.
	/// Use await progress.done() to wait for completion.
	#[napi]
	pub async fn convert_to(&self, output: String, options: Option<ConvertOptions>) -> Result<Progress> {
		let progress = Progress::new();
		let progress_arc = Arc::new(progress);

		// Clone everything we need for the async task
		let output_clone = output.clone();
		let options_clone = options.clone();
		let registry_clone = self.registry.clone();
		let reader_arc = self.reader.clone();
		let progress_task = progress_arc.clone();

		// Spawn the conversion task in the background
		tokio::spawn(async move {
			let result = Self::do_convert(
				reader_arc,
				registry_clone,
				output_clone,
				options_clone,
				Some(progress_task.clone()),
			)
			.await;

			match result {
				Ok(()) => progress_task.complete(),
				Err(e) => progress_task.fail(e),
			}
		});

		// Return the Progress object immediately
		// We need to extract the Progress from Arc and clone it
		Ok((*progress_arc).clone())
	}

	/// Internal conversion implementation
	async fn do_convert(
		reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
		registry: ContainerRegistry,
		output: String,
		options: Option<ConvertOptions>,
		progress: Option<Arc<Progress>>,
	) -> anyhow::Result<()> {
		// Emit initial step if progress monitoring is enabled
		if let Some(ref p) = progress {
			p.emit_step("Initializing conversion".to_string());
		}

		let opts = options.unwrap_or(ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		});

		let mut bbox_pyramid: Option<TileBBoxPyramid> = None;

		if opts.min_zoom.is_some() || opts.max_zoom.is_some() || opts.bbox.is_some() {
			let mut pyramid = TileBBoxPyramid::new_full(32);

			if let Some(min) = opts.min_zoom {
				pyramid.set_level_min(min);
			}

			if let Some(max) = opts.max_zoom {
				pyramid.set_level_max(max);
			}

			if let Some(bbox_vec) = opts.bbox {
				if bbox_vec.len() != 4 {
					return Err(anyhow::anyhow!(
						"bbox must contain exactly 4 numbers [west, south, east, north]"
					));
				}
				let geo_bbox = GeoBBox::try_from(bbox_vec)?;
				pyramid.intersect_geo_bbox(&geo_bbox)?;

				if let Some(border) = opts.bbox_border {
					pyramid.add_border(border, border, border, border);
				}
			}

			bbox_pyramid = Some(pyramid);
		}

		let reader_lock = reader.lock().await;

		let tile_compression = if let Some(ref comp_str) = opts.compress {
			parse_compression(comp_str).ok_or_else(|| {
				anyhow::anyhow!(
					"Invalid compression '{}'. Use 'gzip', 'brotli', or 'uncompressed'",
					comp_str
				)
			})?
		} else {
			reader_lock.parameters().tile_compression
		};

		let params = TilesConverterParameters {
			bbox_pyramid,
			tile_compression: Some(tile_compression),
			flip_y: opts.flip_y.unwrap_or(false),
			swap_xy: opts.swap_xy.unwrap_or(false),
		};

		let output_path = std::path::PathBuf::from(&output);
		let source_name = reader_lock.source_name().to_string();

		// Release the lock before re-opening
		drop(reader_lock);

		if let Some(ref p) = progress {
			p.emit_step("Reading tiles".to_string());
		}

		// Clone the reader by re-opening from source
		let reader_clone = registry.get_reader_from_str(&source_name).await?;

		// Create a processing config with progress monitoring if enabled
		let config = if let Some(ref p) = progress {
			let progress_callback = ProgressCallback::new("converting tiles", 1000, p.clone());
			let progress_bar = progress_callback.progress_bar().clone();

			ProcessingConfig {
				cache_type: versatiles_container::CacheType::new_memory(),
				progress_bar: Some(progress_bar),
			}
		} else {
			ProcessingConfig::default()
		};

		// Use the new function that accepts a ProcessingConfig
		versatiles_container::convert_tiles_container_with_config(reader_clone, params, &output_path, registry, config)
			.await?;

		Ok(())
	}

	/// Probe the container to get detailed information
	///
	/// depth: "shallow", "container", "tiles", or "tile-contents"
	#[napi]
	pub async fn probe(&self, _depth: Option<String>) -> Result<ProbeResult> {
		let reader = self.reader.lock().await;

		Ok(ProbeResult {
			source_name: reader.source_name().to_string(),
			container_name: reader.container_name().to_string(),
			tile_json: reader.tilejson().as_string(),
			parameters: ReaderParameters::from(reader.parameters()),
		})
	}
}
