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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_compression_gzip() {
		let result = parse_compression("gzip");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Gzip));
	}

	#[test]
	fn test_parse_compression_gzip_uppercase() {
		let result = parse_compression("GZIP");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Gzip));
	}

	#[test]
	fn test_parse_compression_brotli() {
		let result = parse_compression("brotli");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Brotli));
	}

	#[test]
	fn test_parse_compression_brotli_mixed_case() {
		let result = parse_compression("BrOtLi");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Brotli));
	}

	#[test]
	fn test_parse_compression_uncompressed() {
		let result = parse_compression("uncompressed");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Uncompressed));
	}

	#[test]
	fn test_parse_compression_none() {
		let result = parse_compression("none");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Uncompressed));
	}

	#[test]
	fn test_parse_compression_none_uppercase() {
		let result = parse_compression("NONE");
		assert!(result.is_ok());
		assert!(matches!(result.unwrap(), TileCompression::Uncompressed));
	}

	#[test]
	fn test_parse_compression_invalid() {
		let result = parse_compression("invalid");
		assert!(result.is_err());
		let err = result.unwrap_err();
		assert!(err.reason.contains("Invalid compression"));
		assert!(err.reason.contains("invalid"));
	}

	#[test]
	fn test_parse_compression_empty() {
		let result = parse_compression("");
		assert!(result.is_err());
	}

	#[test]
	fn test_parse_compression_lz4() {
		let result = parse_compression("lz4");
		assert!(result.is_err());
		let err = result.unwrap_err();
		assert!(err.reason.contains("Invalid compression"));
	}
}
