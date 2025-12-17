use crate::{
	napi_result,
	progress::{MessageData, ProgressData},
	types::{ConvertOptions, ProbeResult, ReaderParameters, parse_compression},
};
use napi::{
	bindgen_prelude::*,
	threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::Mutex;
use versatiles_container::{ContainerRegistry, ProcessingConfig, TilesConverterParameters, TilesReaderTrait};
use versatiles_core::{GeoBBox, TileBBoxPyramid, TileCoord as RustTileCoord};

/// Container reader for accessing tile data from various formats
#[napi]
pub struct ContainerReader {
	reader: Arc<Mutex<Box<dyn TilesReaderTrait>>>,
	registry: Arc<ContainerRegistry>,
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
			registry: Arc::new(registry),
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

	/// Convert this container to another format with optional progress monitoring
	///
	/// Accepts optional callbacks for progress monitoring:
	/// - on_progress: Called with progress updates (position, percentage, speed, eta)
	/// - on_message: Called with messages (type: "step" | "warning" | "error", message: string)
	///
	/// Returns a Promise that resolves when the conversion is complete.
	#[napi]
	pub async fn convert_to(
		&self,
		output: String,
		options: Option<ConvertOptions>,
		on_progress: Option<ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false, true>>,
		on_message: Option<ThreadsafeFunction<MessageData, Unknown<'static>, MessageData, Status, false, true>>,
	) -> Result<()> {
		// Call do_convert directly and await it
		napi_result!(self.do_convert(output, options, on_progress, on_message).await)?;

		Ok(())
	}

	/// Internal conversion implementation
	async fn do_convert(
		&self,
		output: String,
		options: Option<ConvertOptions>,
		on_progress: Option<ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false, true>>,
		on_message: Option<ThreadsafeFunction<MessageData, Unknown<'static>, MessageData, Status, false, true>>,
	) -> anyhow::Result<()> {
		// Emit initial step if message callback is provided
		if let Some(ref cb) = on_message {
			let _ = cb.call(
				MessageData {
					msg_type: "step".to_string(),
					message: "Initializing conversion".to_string(),
				},
				ThreadsafeFunctionCallMode::NonBlocking,
			);
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

		let reader_lock = self.reader.lock().await;

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

		if let Some(ref cb) = on_message {
			let _ = cb.call(
				MessageData {
					msg_type: "step".to_string(),
					message: "Reading tiles".to_string(),
				},
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}

		// Clone the reader by re-opening from source
		let reader_clone = self.registry.get_reader_from_str(&source_name).await?;

		// Create a processing config with progress monitoring if enabled
		let config = if let Some(cb) = on_progress {
			let progress_bar = versatiles_core::progress::ProgressBar::new("converting tiles", 1000);

			// Set callback to emit progress events
			// Wrap in Arc to share between multiple callback invocations
			let cb_arc = std::sync::Arc::new(cb);
			progress_bar.set_callback(move |data| {
				// Convert from versatiles_core::progress::ProgressData to our ProgressData
				let js_data = ProgressData {
					position: data.position as f64,
					total: data.total as f64,
					percentage: data.percentage,
					speed: data.speed,
					eta: data.eta,
					message: Some(data.message),
				};
				// Ignore the result since we're in a callback
				let _ = cb_arc.call(js_data, ThreadsafeFunctionCallMode::NonBlocking);
			});

			ProcessingConfig {
				cache_type: versatiles_container::CacheType::new_memory(),
				progress_bar: Some(progress_bar),
			}
		} else {
			ProcessingConfig::default()
		};

		// Use the new function that accepts a ProcessingConfig
		versatiles_container::convert_tiles_container_with_config(
			reader_clone,
			params,
			&output_path,
			self.registry.clone(),
			Arc::new(config),
		)
		.await?;

		// Emit final completion message
		if let Some(ref cb) = on_message {
			let _ = cb.call(
				MessageData {
					msg_type: "step".to_string(),
					message: "Conversion complete".to_string(),
				},
				ThreadsafeFunctionCallMode::NonBlocking,
			);
		}

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
