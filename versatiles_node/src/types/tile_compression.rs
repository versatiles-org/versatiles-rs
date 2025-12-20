use versatiles_core::TileCompression;

/// Helper to parse compression string
pub fn parse_compression(s: &str) -> napi::Result<TileCompression> {
	match s.to_lowercase().as_str() {
		"gzip" => Ok(TileCompression::Gzip),
		"brotli" => Ok(TileCompression::Brotli),
		"uncompressed" | "none" => Ok(TileCompression::Uncompressed),
		_ => Err(napi::Error::from_reason(format!(
			"Invalid compression '{s}'. Use 'gzip', 'brotli', or 'uncompressed'"
		))),
	}
}
