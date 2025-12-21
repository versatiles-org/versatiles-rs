//! Tile conversion functionality
//!
//! This module provides the `convert` function for converting tiles between different
//! container formats with support for filtering, transformation, and real-time progress
//! monitoring.
//!
//! ## Conversion Features
//!
//! - **Format conversion**: Convert between .versatiles, .mbtiles, .pmtiles, .tar
//! - **Zoom filtering**: Select specific zoom level ranges
//! - **Geographic filtering**: Extract tiles within a bounding box
//! - **Compression**: Change tile compression (gzip, brotli, uncompressed)
//! - **Transformations**: Flip Y-axis or swap X/Y coordinates
//! - **Progress monitoring**: Real-time progress updates and messages

use crate::{
	napi_result,
	progress::{MessageData, ProgressData},
	runtime::create_runtime,
	types::{ConvertOptions, parse_compression},
};
use napi::{
	bindgen_prelude::*,
	threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use std::sync::Arc;
use versatiles_container::{
	TilesConverterParameters,
	runtime::{Event, TilesRuntime},
};
use versatiles_core::{GeoBBox, TileBBoxPyramid};

/// Convert tiles from one container format to another
///
/// Converts tiles between different container formats with optional filtering,
/// transformation, and compression changes. Supports real-time progress monitoring
/// through callback functions.
///
/// # Arguments
///
/// * `input` - Path or URL to the input tile container
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
/// - Input file/URL doesn't exist or is inaccessible
/// - Output path is invalid or not writable
/// - Bbox coordinates are invalid (must be `[west, south, east, north]`)
/// - Compression format is not recognized
/// - An I/O error occurs during conversion
///
/// # Examples
///
/// ```javascript
/// // Simple conversion
/// await convert('input.mbtiles', 'output.versatiles');
///
/// // Convert with compression
/// await convert('input.pmtiles', 'output.versatiles', {
///   compress: 'brotli'
/// });
///
/// // Convert specific area and zoom range
/// await convert('world.mbtiles', 'europe.versatiles', {
///   minZoom: 0,
///   maxZoom: 14,
///   bbox: [-10, 35, 40, 70], // Europe
///   bboxBorder: 1
/// });
///
/// // With progress monitoring
/// await convert('input.tar', 'output.versatiles', null,
///   (progress) => {
///     console.log(`${progress.percentage.toFixed(1)}% complete`);
///     console.log(`Speed: ${progress.speed.toFixed(0)} tiles/sec`);
///     console.log(`ETA: ${new Date(progress.eta)}`);
///   },
///   (type, message) => {
///     if (type === 'error') console.error(message);
///     else if (type === 'warning') console.warn(message);
///     else console.log(message);
///   }
/// );
/// ```
#[napi]
pub async fn convert(
	input: String,
	output: String,
	options: Option<ConvertOptions>,
	on_progress: Option<ThreadsafeFunction<ProgressData, Unknown<'static>, ProgressData, Status, false, true>>,
	on_message: Option<ThreadsafeFunction<MessageData, Unknown<'static>, MessageData, Status, false, true>>,
) -> Result<()> {
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
				return Err(napi::Error::from_reason(
					"bbox must contain exactly 4 numbers [west, south, east, north]",
				));
			}
			let geo_bbox = napi_result!(GeoBBox::try_from(bbox_vec))?;
			napi_result!(pyramid.intersect_geo_bbox(&geo_bbox))?;

			if let Some(border) = opts.bbox_border {
				pyramid.add_border(border, border, border, border);
			}
		}

		bbox_pyramid = Some(pyramid);
	}

	let runtime = create_runtime();
	let reader = napi_result!(runtime.get_reader_from_str(&input).await)?;

	let tile_compression = if let Some(ref comp_str) = opts.compress {
		Some(parse_compression(comp_str)?)
	} else {
		None
	};

	let params = TilesConverterParameters {
		bbox_pyramid,
		tile_compression,
		flip_y: opts.flip_y.unwrap_or(false),
		swap_xy: opts.swap_xy.unwrap_or(false),
	};

	let output_path = std::path::PathBuf::from(&output);

	// Create a new runtime for this conversion with event bridging to JavaScript
	let runtime = TilesRuntime::default();

	// Bridge progress events to JavaScript callback
	if let Some(cb) = on_progress {
		let cb_arc = Arc::new(cb);
		runtime.events().subscribe(move |event| {
			if let Event::Progress { data, .. } = event {
				// Convert Rust ProgressData to Node.js ProgressData
				let js_data = ProgressData::from(data);
				let _ = cb_arc.call(js_data, ThreadsafeFunctionCallMode::NonBlocking);
			}
		});
	}

	// Bridge message events (step, warning, error) to JavaScript callback
	if let Some(cb) = on_message {
		let cb_arc = Arc::new(cb);
		runtime.events().subscribe(move |event| {
			let (msg_type, message): (&str, String) = match event {
				Event::Step { message } => ("step", message.clone()),
				Event::Warning { message } => ("warning", message.clone()),
				Event::Error { message } => ("error", message.clone()),
				_ => return,
			};
			let js_msg = MessageData {
				msg_type: msg_type.to_string(),
				message,
			};
			let _ = cb_arc.call(js_msg, ThreadsafeFunctionCallMode::NonBlocking);
		});
	}

	// Convert tiles using the new API
	napi_result!(versatiles_container::convert_tiles_container(reader, params, &output_path, runtime).await)?;

	Ok(())
}
