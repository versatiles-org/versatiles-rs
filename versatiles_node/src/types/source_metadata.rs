use napi_derive::napi;
use versatiles_container::TileSourceMetadata;

/// Tile source metadata describing output characteristics
#[napi(object)]
#[derive(Clone)]
pub struct SourceMetadata {
	/// Tile format (e.g., "png", "jpg", "mvt")
	pub tile_format: String,
	/// Tile compression (e.g., "gzip", "brotli", "uncompressed")
	pub tile_compression: String,
	/// Minimum zoom level available
	pub min_zoom: u8,
	/// Maximum zoom level available
	pub max_zoom: u8,
}

impl From<&TileSourceMetadata> for SourceMetadata {
	fn from(params: &TileSourceMetadata) -> Self {
		Self {
			tile_format: format!("{:?}", params.tile_format).to_lowercase(),
			tile_compression: format!("{:?}", params.tile_compression).to_lowercase(),
			min_zoom: params.bbox_pyramid.get_level_min().unwrap_or(0),
			max_zoom: params.bbox_pyramid.get_level_max().unwrap_or(0),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_container::Traversal;
	use versatiles_core::{TileBBoxPyramid, TileCompression, TileFormat};

	fn create_test_parameters(
		format: TileFormat,
		compression: TileCompression,
		min_zoom: u8,
		max_zoom: u8,
	) -> TileSourceMetadata {
		let mut bbox_pyramid = TileBBoxPyramid::new_full();
		bbox_pyramid.set_level_min(min_zoom);
		bbox_pyramid.set_level_max(max_zoom);

		TileSourceMetadata::new(format, compression, bbox_pyramid, Traversal::ANY)
	}

	#[test]
	fn test_from_tiles_reader_parameters_png_uncompressed() {
		let params = create_test_parameters(TileFormat::PNG, TileCompression::Uncompressed, 0, 14);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.tile_format, "png");
		assert_eq!(reader_params.tile_compression, "uncompressed");
		assert_eq!(reader_params.min_zoom, 0);
		assert_eq!(reader_params.max_zoom, 14);
	}

	#[test]
	fn test_from_tiles_reader_parameters_jpg_gzip() {
		let params = create_test_parameters(TileFormat::JPG, TileCompression::Gzip, 2, 10);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.tile_format, "jpg");
		assert_eq!(reader_params.tile_compression, "gzip");
		assert_eq!(reader_params.min_zoom, 2);
		assert_eq!(reader_params.max_zoom, 10);
	}

	#[test]
	fn test_from_tiles_reader_parameters_webp_brotli() {
		let params = create_test_parameters(TileFormat::WEBP, TileCompression::Brotli, 5, 18);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.tile_format, "webp");
		assert_eq!(reader_params.tile_compression, "brotli");
		assert_eq!(reader_params.min_zoom, 5);
		assert_eq!(reader_params.max_zoom, 18);
	}

	#[test]
	fn test_from_tiles_reader_parameters_mvt_gzip() {
		let params = create_test_parameters(TileFormat::MVT, TileCompression::Gzip, 0, 14);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.tile_format, "mvt");
		assert_eq!(reader_params.tile_compression, "gzip");
		assert_eq!(reader_params.min_zoom, 0);
		assert_eq!(reader_params.max_zoom, 14);
	}

	#[test]
	fn test_from_tiles_reader_parameters_same_min_max_zoom() {
		let params = create_test_parameters(TileFormat::PNG, TileCompression::Uncompressed, 5, 5);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.min_zoom, 5);
		assert_eq!(reader_params.max_zoom, 5);
	}

	#[test]
	fn test_from_tiles_reader_parameters_max_zoom_range() {
		let params = create_test_parameters(TileFormat::PNG, TileCompression::Uncompressed, 0, 31);
		let reader_params = SourceMetadata::from(&params);

		assert_eq!(reader_params.min_zoom, 0);
		assert_eq!(reader_params.max_zoom, 30);
	}

	#[test]
	fn test_format_names_are_lowercase() {
		let formats = [
			(TileFormat::PNG, "png"),
			(TileFormat::JPG, "jpg"),
			(TileFormat::WEBP, "webp"),
			(TileFormat::MVT, "mvt"),
		];

		for (format, expected) in formats {
			let params = create_test_parameters(format, TileCompression::Uncompressed, 0, 14);
			let reader_params = SourceMetadata::from(&params);
			assert_eq!(reader_params.tile_format, expected);
		}
	}

	#[test]
	fn test_compression_names_are_lowercase() {
		let compressions = [
			(TileCompression::Uncompressed, "uncompressed"),
			(TileCompression::Gzip, "gzip"),
			(TileCompression::Brotli, "brotli"),
		];

		for (compression, expected) in compressions {
			let params = create_test_parameters(TileFormat::PNG, compression, 0, 14);
			let reader_params = SourceMetadata::from(&params);
			assert_eq!(reader_params.tile_compression, expected);
		}
	}

	#[test]
	fn test_reader_parameters_clone() {
		let params = SourceMetadata {
			tile_format: "png".to_string(),
			tile_compression: "gzip".to_string(),
			min_zoom: 5,
			max_zoom: 15,
		};

		let cloned = params.clone();
		assert_eq!(cloned.tile_format, params.tile_format);
		assert_eq!(cloned.tile_compression, params.tile_compression);
		assert_eq!(cloned.min_zoom, params.min_zoom);
		assert_eq!(cloned.max_zoom, params.max_zoom);
	}
}
