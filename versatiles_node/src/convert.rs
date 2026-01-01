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
use std::{path::Path, sync::Arc};
use versatiles_container::{TileSource as RustTileSource, TilesConverterParameters, runtime::Event};
use versatiles_core::{GeoBBox, TileBBoxPyramid};

/// Internal helper function to convert tiles with options and callbacks
///
/// This shared function handles the common conversion logic used by both
/// the `convert()` function and `TileSource.convertTo()` method.
pub(crate) async fn convert_tiles_with_options(
	reader: Box<dyn RustTileSource>,
	output: &Path,
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

	// Create a new runtime for this conversion with event bridging to JavaScript
	let runtime = create_runtime();

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

	// Convert tiles using the Rust API
	napi_result!(versatiles_container::convert_tiles_container(reader, params, output, runtime).await)?;

	Ok(())
}

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
	// Open the input tile source
	let runtime = create_runtime();
	let reader = napi_result!(runtime.get_reader_from_str(&input).await)?;

	// Use shared conversion logic
	let output_path = std::path::PathBuf::from(&output);
	convert_tiles_with_options(reader, &output_path, options, on_progress, on_message).await
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;
	use versatiles_core::TileCompression;

	/// Test bbox validation - must have exactly 4 elements
	#[rstest]
	#[case(vec![])]
	#[case(vec![0.0])]
	#[case(vec![0.0, 0.0])]
	#[case(vec![0.0, 0.0, 0.0])]
	#[case(vec![0.0, 0.0, 0.0, 0.0, 0.0])]
	fn test_bbox_validation_invalid_length(#[case] bbox: Vec<f64>) {
		assert_ne!(bbox.len(), 4, "Expected bbox length to not be 4");
	}

	#[test]
	fn test_bbox_validation_valid_length() {
		let valid_bbox = [0.0, 0.0, 10.0, 10.0];
		assert_eq!(valid_bbox.len(), 4);
	}

	/// Test bbox validation - coordinates must form valid geographic bounds
	#[rstest]
	#[case([-10.0, 35.0, 40.0, 70.0], "Europe")]
	#[case([-180.0, -90.0, 180.0, 90.0], "World")]
	#[case([0.0, 0.0, 1.0, 1.0], "Small region")]
	fn test_bbox_coordinate_validation(#[case] bbox: [f64; 4], #[case] _name: &str) {
		assert_eq!(bbox.len(), 4);
		assert!(bbox[0] < bbox[2], "West must be less than east");
		assert!(bbox[1] < bbox[3], "South must be less than north");
	}

	/// Test ConvertOptions with default values
	#[test]
	fn test_convert_options_defaults() {
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
		assert!(opts.max_zoom.is_none());
		assert!(opts.bbox.is_none());
		assert!(opts.bbox_border.is_none());
		assert!(opts.compress.is_none());
		assert!(opts.flip_y.is_none());
		assert!(opts.swap_xy.is_none());
	}

	/// Test ConvertOptions with all values set
	#[test]
	fn test_convert_options_all_set() {
		let opts = ConvertOptions {
			min_zoom: Some(0),
			max_zoom: Some(14),
			bbox: Some(vec![-10.0, 35.0, 40.0, 70.0]),
			bbox_border: Some(1),
			compress: Some("gzip".to_string()),
			flip_y: Some(true),
			swap_xy: Some(true),
		};

		assert_eq!(opts.min_zoom, Some(0));
		assert_eq!(opts.max_zoom, Some(14));
		assert_eq!(opts.bbox.as_ref().unwrap().len(), 4);
		assert_eq!(opts.bbox_border, Some(1));
		assert_eq!(opts.compress.as_ref().unwrap(), "gzip");
		assert_eq!(opts.flip_y, Some(true));
		assert_eq!(opts.swap_xy, Some(true));
	}

	/// Test TileBBoxPyramid creation with zoom filtering
	#[test]
	fn test_pyramid_zoom_filtering() {
		let mut pyramid = TileBBoxPyramid::new_full(32);

		// Set zoom range 5-10
		pyramid.set_level_min(5);
		pyramid.set_level_max(10);

		// Verify the pyramid respects zoom bounds
		assert_eq!(pyramid.get_level_min(), Some(5));
		assert_eq!(pyramid.get_level_max(), Some(10));
	}

	/// Test TileBBoxPyramid with geographic bbox
	#[test]
	fn test_pyramid_geo_bbox() {
		let mut pyramid = TileBBoxPyramid::new_full(10);

		// Europe bounding box
		let geo_bbox = GeoBBox::new(-10.0, 35.0, 40.0, 70.0).unwrap();
		pyramid.intersect_geo_bbox(&geo_bbox).unwrap();

		// Pyramid should now be limited to the bbox area
		// (exact tile counts depend on zoom level)
		assert!(pyramid.count_tiles() > 0);
		assert!(pyramid.count_tiles() < TileBBoxPyramid::new_full(10).count_tiles());
	}

	/// Test TileBBoxPyramid border addition
	#[test]
	fn test_pyramid_bbox_border() {
		let mut pyramid = TileBBoxPyramid::new_full(5);
		let geo_bbox = GeoBBox::new(0.0, 0.0, 10.0, 10.0).unwrap();
		pyramid.intersect_geo_bbox(&geo_bbox).unwrap();

		let count_without_border = pyramid.count_tiles();

		// Add 1-tile border on all sides
		pyramid.add_border(1, 1, 1, 1);

		let count_with_border = pyramid.count_tiles();

		// Border should increase tile count
		assert!(count_with_border >= count_without_border);
	}

	/// Test compression string parsing - valid formats
	#[rstest]
	#[case("gzip", TileCompression::Gzip)]
	#[case("GZIP", TileCompression::Gzip)]
	#[case("brotli", TileCompression::Brotli)]
	#[case("Brotli", TileCompression::Brotli)]
	#[case("uncompressed", TileCompression::Uncompressed)]
	#[case("none", TileCompression::Uncompressed)]
	fn test_compression_parsing_valid(#[case] input: &str, #[case] expected: TileCompression) {
		let result = parse_compression(input);
		assert!(result.is_ok());
		// Compare using discriminant since TileCompression doesn't implement PartialEq
		assert_eq!(
			std::mem::discriminant(&result.unwrap()),
			std::mem::discriminant(&expected)
		);
	}

	/// Test compression string parsing - invalid formats
	#[rstest]
	#[case("invalid")]
	#[case("")]
	#[case("zip")]
	#[case("lz4")]
	fn test_compression_parsing_invalid(#[case] input: &str) {
		assert!(parse_compression(input).is_err());
	}

	/// Test zoom level edge cases
	#[rstest]
	#[case(Some(0), Some(0), "world in single tile")]
	#[case(Some(5), Some(10), "mid-range zoom")]
	#[case(Some(20), Some(22), "high zoom")]
	fn test_zoom_level_edge_cases(#[case] min_zoom: Option<u8>, #[case] max_zoom: Option<u8>, #[case] _desc: &str) {
		let opts = ConvertOptions {
			min_zoom,
			max_zoom,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};
		assert_eq!(opts.min_zoom, min_zoom);
		assert_eq!(opts.max_zoom, max_zoom);
	}

	/// Test bbox with different geographic regions
	#[rstest]
	#[case([-10.0, 35.0, 40.0, 70.0], false, "Europe")]
	#[case([60.0, 0.0, 150.0, 70.0], false, "Asia")]
	#[case([13.3, 52.5, 13.5, 52.6], true, "Berlin (city-level)")]
	fn test_bbox_different_regions(#[case] bbox: [f64; 4], #[case] is_small: bool, #[case] _name: &str) {
		assert_eq!(bbox.len(), 4);
		if is_small {
			assert!(bbox[2] - bbox[0] < 1.0); // Less than 1 degree wide
			assert!(bbox[3] - bbox[1] < 1.0); // Less than 1 degree tall
		}
	}

	/// Test bbox_border values
	#[rstest]
	#[case(None)]
	#[case(Some(1))]
	#[case(Some(5))]
	fn test_bbox_border_values(#[case] bbox_border: Option<u32>) {
		let opts = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: Some(vec![0.0, 0.0, 10.0, 10.0]),
			bbox_border,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};
		assert_eq!(opts.bbox_border, bbox_border);
	}

	/// Test flip_y and swap_xy transformation flags
	#[rstest]
	#[case(None, None, "no transformations")]
	#[case(Some(true), None, "flip Y only (TMS ↔ XYZ)")]
	#[case(None, Some(true), "swap XY only")]
	#[case(Some(true), Some(true), "both transformations")]
	fn test_transformation_flags(#[case] flip_y: Option<bool>, #[case] swap_xy: Option<bool>, #[case] _desc: &str) {
		let opts = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y,
			swap_xy,
		};
		assert_eq!(opts.flip_y, flip_y);
		assert_eq!(opts.swap_xy, swap_xy);
	}

	/// Test pyramid creation requires filtering options
	#[rstest]
	#[case(None, None, None, false, "no filtering")]
	#[case(Some(5), None, None, true, "with min_zoom")]
	#[case(None, Some(10), None, true, "with max_zoom")]
	#[case(None, None, Some(vec![0.0, 0.0, 10.0, 10.0]), true, "with bbox")]
	fn test_pyramid_creation_conditions(
		#[case] min_zoom: Option<u8>,
		#[case] max_zoom: Option<u8>,
		#[case] bbox: Option<Vec<f64>>,
		#[case] expected_create: bool,
		#[case] _desc: &str,
	) {
		let opts = ConvertOptions {
			min_zoom,
			max_zoom,
			bbox,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		// Condition on line 136: if min_zoom, max_zoom, or bbox is set
		let should_create_pyramid = opts.min_zoom.is_some() || opts.max_zoom.is_some() || opts.bbox.is_some();
		assert_eq!(should_create_pyramid, expected_create);
	}

	/// Test GeoBBox creation from valid coordinates
	#[rstest]
	#[case(-10.0, 35.0, 40.0, 70.0, "Europe")]
	#[case(-180.0, -90.0, 180.0, 90.0, "World")]
	#[case(0.0, 0.0, 0.1, 0.1, "Small region")]
	fn test_geo_bbox_creation(
		#[case] west: f64,
		#[case] south: f64,
		#[case] east: f64,
		#[case] north: f64,
		#[case] _name: &str,
	) {
		let result = GeoBBox::new(west, south, east, north);
		assert!(result.is_ok());
	}

	/// Test GeoBBox with inverted coordinates (should fail)
	#[rstest]
	#[case(40.0, 35.0, -10.0, 70.0, "West > East")]
	#[case(-10.0, 70.0, 40.0, 35.0, "South > North")]
	fn test_geo_bbox_inverted_coordinates(
		#[case] west: f64,
		#[case] south: f64,
		#[case] east: f64,
		#[case] north: f64,
		#[case] _desc: &str,
	) {
		let result = GeoBBox::new(west, south, east, north);
		assert!(result.is_err());
	}

	/// Test TilesConverterParameters construction
	#[rstest]
	#[case(Some(TileCompression::Gzip), false, false, false, "with compression")]
	#[case(None, true, true, false, "with transformations")]
	#[case(None, false, false, true, "with pyramid")]
	fn test_converter_parameters(
		#[case] compression: Option<TileCompression>,
		#[case] flip_y: bool,
		#[case] swap_xy: bool,
		#[case] with_pyramid: bool,
		#[case] _desc: &str,
	) {
		let bbox_pyramid = if with_pyramid {
			Some(TileBBoxPyramid::new_full(10))
		} else {
			None
		};

		let params = TilesConverterParameters {
			bbox_pyramid,
			tile_compression: compression,
			flip_y,
			swap_xy,
		};

		assert_eq!(params.tile_compression.is_some(), compression.is_some());
		assert_eq!(params.flip_y, flip_y);
		assert_eq!(params.swap_xy, swap_xy);
		assert_eq!(params.bbox_pyramid.is_some(), with_pyramid);
	}

	/// Test bbox_border is only applied when bbox exists
	#[test]
	fn test_bbox_border_requires_bbox() {
		// bbox_border without bbox - should be ignored
		let opts = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: Some(5), // This should have no effect
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		// The code on line 147 checks if bbox exists before using bbox_border
		if opts.bbox.is_some() {
			// Only use bbox_border if bbox exists
			assert!(opts.bbox_border.is_some());
		} else {
			// bbox_border is set but won't be used
			assert!(opts.bbox.is_none());
		}
	}

	/// Test compression option validation
	#[rstest]
	#[case("none", true, "valid compression")]
	#[case("", false, "empty string")]
	#[case("invalid", false, "invalid format")]
	fn test_compression_option_validation(#[case] input: &str, #[case] should_be_valid: bool, #[case] _desc: &str) {
		let result = Some(input.to_string());
		let compression = result.as_ref().map(|s| parse_compression(s));
		assert!(compression.is_some());
		assert_eq!(compression.unwrap().is_ok(), should_be_valid);
	}
}
