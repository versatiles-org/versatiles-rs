use napi_derive::napi;

/// Options for tile conversion
///
/// Configure how tiles are filtered, transformed, and compressed during conversion.
/// All fields are optional - only specify the options you want to apply.
#[napi(object)]
#[derive(Clone)]
pub struct ConvertOptions {
	/// Minimum zoom level to include (0-32)
	///
	/// Tiles with zoom levels below this value will be excluded from the output.
	/// Must be less than or equal to `max_zoom` if both are specified.
	///
	/// **Default:** No minimum filter (all zoom levels included)
	pub min_zoom: Option<u8>,

	/// Maximum zoom level to include (0-32)
	///
	/// Tiles with zoom levels above this value will be excluded from the output.
	/// Must be greater than or equal to `min_zoom` if both are specified.
	///
	/// **Default:** No maximum filter (all zoom levels included)
	pub max_zoom: Option<u8>,

	/// Geographic bounding box filter in WGS84 coordinates
	///
	/// Must be an array of exactly 4 numbers: `[west, south, east, north]`
	/// - **west**: Minimum longitude (-180 to 180)
	/// - **south**: Minimum latitude (-85.0511 to 85.0511)
	/// - **east**: Maximum longitude (-180 to 180)
	/// - **north**: Maximum latitude (-85.0511 to 85.0511)
	///
	/// Only tiles that intersect this bounding box will be included.
	/// Coordinate system is WGS84 (EPSG:4326).
	///
	/// **Example:** `[13.0, 52.0, 14.0, 53.0]` for Berlin area
	///
	/// **Default:** No bounding box filter (world coverage)
	pub bbox: Option<Vec<f64>>,

	/// Number of extra tiles to include around the bounding box
	///
	/// Adds a buffer of tiles around the `bbox` at each zoom level.
	/// The value is in tile units, not geographic degrees.
	/// Useful for ensuring smooth rendering at the edges of the bounding box.
	///
	/// **Example:** A value of `1` adds one tile border on each side
	///
	/// **Default:** `0` (no border)
	pub bbox_border: Option<u32>,

	/// Output tile compression format
	///
	/// Valid values:
	/// - `"gzip"`: Good compression ratio, widely supported
	/// - `"brotli"`: Better compression than gzip, modern browsers only
	/// - `"uncompressed"`: No compression, fastest but largest files
	///
	/// **Default:** Preserves the source compression format
	pub compress: Option<String>,

	/// Flip tiles vertically (swap TMS ↔ XYZ coordinate systems)
	///
	/// - **TMS** (Tile Map Service): Y increases from south to north (origin at bottom-left)
	/// - **XYZ**: Y increases from north to south (origin at top-left)
	///
	/// Set to `true` to convert between these coordinate systems.
	///
	/// **Default:** `false` (no flipping)
	pub flip_y: Option<bool>,

	/// Swap X and Y tile coordinates
	///
	/// Exchanges the column (x) and row (y) coordinates of each tile.
	/// Rarely needed except for specialized coordinate transformations.
	///
	/// **Default:** `false` (no swapping)
	pub swap_xy: Option<bool>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_convert_options_clone() {
		let options = ConvertOptions {
			min_zoom: Some(5),
			max_zoom: Some(15),
			bbox: Some(vec![-10.0, 35.0, 40.0, 70.0]),
			bbox_border: Some(1),
			compress: Some("brotli".to_string()),
			flip_y: Some(true),
			swap_xy: Some(false),
		};

		let cloned = options.clone();
		assert_eq!(cloned.min_zoom, options.min_zoom);
		assert_eq!(cloned.max_zoom, options.max_zoom);
		assert_eq!(cloned.bbox, options.bbox);
		assert_eq!(cloned.bbox_border, options.bbox_border);
		assert_eq!(cloned.compress, options.compress);
		assert_eq!(cloned.flip_y, options.flip_y);
		assert_eq!(cloned.swap_xy, options.swap_xy);
	}

	#[test]
	fn test_convert_options_default_none() {
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		assert!(options.min_zoom.is_none());
		assert!(options.max_zoom.is_none());
		assert!(options.bbox.is_none());
		assert!(options.bbox_border.is_none());
		assert!(options.compress.is_none());
		assert!(options.flip_y.is_none());
		assert!(options.swap_xy.is_none());
	}

	#[test]
	fn test_convert_options_with_zoom_range() {
		let options = ConvertOptions {
			min_zoom: Some(0),
			max_zoom: Some(14),
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		assert_eq!(options.min_zoom, Some(0));
		assert_eq!(options.max_zoom, Some(14));
	}

	#[test]
	fn test_convert_options_with_bbox() {
		let bbox = vec![-180.0, -85.0, 180.0, 85.0];
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: Some(bbox.clone()),
			bbox_border: Some(2),
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		assert_eq!(options.bbox, Some(bbox));
		assert_eq!(options.bbox_border, Some(2));
	}

	#[test]
	fn test_convert_options_with_compression() {
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: Some("gzip".to_string()),
			flip_y: None,
			swap_xy: None,
		};

		assert_eq!(options.compress, Some("gzip".to_string()));
	}

	#[test]
	fn test_convert_options_with_transformations() {
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: None,
			bbox_border: None,
			compress: None,
			flip_y: Some(true),
			swap_xy: Some(true),
		};

		assert_eq!(options.flip_y, Some(true));
		assert_eq!(options.swap_xy, Some(true));
	}

	#[test]
	fn test_convert_options_mixed_configuration() {
		let options = ConvertOptions {
			min_zoom: Some(5),
			max_zoom: Some(10),
			bbox: Some(vec![13.0, 52.0, 14.0, 53.0]),
			bbox_border: Some(0),
			compress: Some("brotli".to_string()),
			flip_y: Some(false),
			swap_xy: Some(true),
		};

		assert_eq!(options.min_zoom, Some(5));
		assert_eq!(options.max_zoom, Some(10));
		assert!(options.bbox.is_some());
		assert_eq!(options.bbox_border, Some(0));
		assert_eq!(options.compress, Some("brotli".to_string()));
		assert_eq!(options.flip_y, Some(false));
		assert_eq!(options.swap_xy, Some(true));
	}

	#[test]
	fn test_convert_options_europe_bbox() {
		let europe_bbox = vec![-10.0, 35.0, 40.0, 70.0];
		let options = ConvertOptions {
			min_zoom: Some(0),
			max_zoom: Some(14),
			bbox: Some(europe_bbox.clone()),
			bbox_border: Some(1),
			compress: Some("brotli".to_string()),
			flip_y: None,
			swap_xy: None,
		};

		if let Some(bbox) = &options.bbox {
			assert_eq!(bbox.len(), 4);
			assert_eq!(bbox[0], -10.0); // west
			assert_eq!(bbox[1], 35.0); // south
			assert_eq!(bbox[2], 40.0); // east
			assert_eq!(bbox[3], 70.0); // north
		} else {
			panic!("bbox should be Some");
		}
	}

	#[test]
	fn test_convert_options_bbox_border_zero() {
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: Some(vec![0.0, 0.0, 1.0, 1.0]),
			bbox_border: Some(0),
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		assert_eq!(options.bbox_border, Some(0));
	}

	#[test]
	fn test_convert_options_large_bbox_border() {
		let options = ConvertOptions {
			min_zoom: None,
			max_zoom: None,
			bbox: Some(vec![0.0, 0.0, 1.0, 1.0]),
			bbox_border: Some(100),
			compress: None,
			flip_y: None,
			swap_xy: None,
		};

		assert_eq!(options.bbox_border, Some(100));
	}
}
