use napi_derive::napi;

/// Options for tile conversion
#[napi(object)]
#[derive(Clone)]
pub struct ConvertOptions {
	/// Minimum zoom level to include
	pub min_zoom: Option<u8>,
	/// Maximum zoom level to include
	pub max_zoom: Option<u8>,
	/// Bounding box [west, south, east, north]
	pub bbox: Option<Vec<f64>>,
	/// Border around bbox in tiles
	pub bbox_border: Option<u32>,
	/// Compression: "gzip", "brotli", or "uncompressed"
	pub compress: Option<String>,
	/// Flip tiles vertically
	pub flip_y: Option<bool>,
	/// Swap x and y coordinates
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
