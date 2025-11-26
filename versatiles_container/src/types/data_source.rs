//! The `DataSource` struct represents a driver-aware wrapper around a `DataLocation`,
//! allowing for parsing and handling of input specifications that may include
//! driver prefixes, standard input, and extension inference.

use super::data_location::DataLocation;
use crate::ContainerRegistry;
use anyhow::{Context, Result, ensure};
use versatiles_core::Blob;

#[derive(Debug, Clone, PartialEq)]
/// Represents a parsed input specification which may include a driver prefix (like `mbtiles:`).
/// It can resolve standard input (`-`) into an in-memory Blob and derives the effective extension
/// that determines which reader to pick.
pub struct DataSource {
	extension: String,      // mbtiles / vpl / ...
	location: DataLocation, // URL, filesystem path, or in-memory blob
}

lazy_static::lazy_static! {
	static ref RE_DRIVER_PREFIX: regex::Regex = regex::Regex::new(r#"(?i)^([a-z]+):(.+)"#).unwrap();
}

impl DataSource {
	/// Returns the effective extension for this data source.
	///
	/// This is typically the driver prefix if one was specified (e.g., `mbtiles`),
	/// otherwise it is inferred from the underlying location's extension.
	pub fn extension(&self) -> &str {
		&self.extension
	}

	/// Returns a reference to the underlying `DataLocation`.
	pub fn location(&self) -> &DataLocation {
		&self.location
	}

	/// Consumes the `DataSource` and returns the underlying `DataLocation`.
	pub fn into_location(self) -> DataLocation {
		self.location
	}

	/// Parses a string specification into a `DataSource`.
	///
	/// The input grammar supports optional driver prefixes (e.g., `mbtiles:`),
	/// standard input (`-`) which is resolved into a Blob (requiring an explicit extension),
	/// and falls back to interpreting the string as a path or URL.
	///
	/// The `registry` is used to validate known driver prefixes.
	pub fn parse(s: &str, registry: &ContainerRegistry) -> Result<Self> {
		let stdin = std::io::stdin();
		let mut stdin_lock = stdin.lock();
		Self::parse_with_stdin(s, registry, &mut stdin_lock)
	}

	/// Internal parsing function that allows injecting a custom `stdin` reader.
	///
	/// This is primarily used for testing to simulate reading from standard input.
	fn parse_with_stdin<R: std::io::Read>(s: &str, registry: &ContainerRegistry, stdin: &mut R) -> Result<Self> {
		let (extension_string, location_string) = if let Some(caps) = RE_DRIVER_PREFIX.captures(s) {
			let prefix = caps.get(1).unwrap().as_str();
			let rest = caps.get(2).unwrap().as_str();
			if registry.supports_reader_extension(prefix) {
				(Some(prefix.to_string()), rest.to_string())
			} else {
				(None, s.to_string())
			}
		} else {
			(None, s.to_string())
		};

		let location = if location_string.trim() == "-" {
			ensure!(
				extension_string.is_some(),
				"When reading from stdin, an explicit extension must be provided (e.g., 'vpl:-')"
			);
			let mut buffer = Vec::new();
			stdin
				.read_to_end(&mut buffer)
				.with_context(|| "Failed to read from stdin")?;
			DataLocation::Blob(Blob::from(buffer))
		} else {
			DataLocation::from(location_string.as_str())
		};

		let extension = extension_string.unwrap_or_else(|| location.extension().unwrap_or(String::from("unknown")));

		Ok(DataSource { extension, location })
	}

	/// Resolves the underlying location relative to a base location.
	///
	/// This delegates to the `DataLocation::resolve` method.
	pub fn resolve(&mut self, base: &DataLocation) -> Result<()> {
		self.location.resolve(base)
	}
}

/// Converts a `DataLocation` into a `DataSource`, inferring the extension from the location.
impl From<DataLocation> for DataSource {
	fn from(location: DataLocation) -> Self {
		let extension = location.extension().unwrap_or(String::from("unknown"));
		DataSource { extension, location }
	}
}

#[cfg(test)]
mod tests {
	use std::{io::Cursor, path::Path};

	use super::*;
	use rstest::rstest;

	fn make_registry() -> ContainerRegistry {
		ContainerRegistry::default()
	}

	#[rstest]
	fn parse_simple_path_uses_path_extension() {
		let registry = make_registry();
		let ds = DataSource::parse("data/example.mbtiles", &registry).unwrap();
		assert_eq!(ds.extension(), "mbtiles");
		match ds.location() {
			DataLocation::Path(path) => {
				assert_eq!(path.file_name().and_then(|s| s.to_str()), Some("example.mbtiles"));
			}
			_ => panic!("Expected Path variant"),
		}
	}

	#[rstest]
	fn parse_url_uses_url_extension() {
		let registry = make_registry();
		let ds = DataSource::parse("https://example.org/tiles/test.tar", &registry).unwrap();
		assert_eq!(ds.extension(), "tar");
		match ds.location() {
			DataLocation::Url(url) => {
				assert!(url.to_string().ends_with("test.tar"));
			}
			_ => panic!("Expected Url variant"),
		}
	}

	#[rstest]
	fn parse_with_known_driver_prefix_uses_prefix_and_rest_as_location() {
		let registry = make_registry();
		let ds = DataSource::parse("mbtiles:raw_data.bin", &registry).unwrap();
		assert_eq!(ds.extension(), "mbtiles");
		match ds.location() {
			DataLocation::Path(path) => {
				assert_eq!(path.file_name().and_then(|s| s.to_str()), Some("raw_data.bin"));
			}
			_ => panic!("Expected Path variant"),
		}
	}

	#[rstest]
	fn parse_with_unknown_driver_prefix_falls_back_to_full_string() {
		let registry = make_registry();
		let ds = DataSource::parse("xxx:some/file.dat", &registry).unwrap();
		let expected_extension = ds.location().extension().unwrap_or_else(|_| "unknown".to_string());
		assert_eq!(ds.extension(), expected_extension);
		// Should not be Blob
		match ds.location() {
			DataLocation::Path(path) => {
				assert_eq!(path.file_name().and_then(|s| s.to_str()), Some("file.dat"));
			}
			DataLocation::Url(url) => {
				assert!(url.to_string().ends_with("file.dat"));
			}
			DataLocation::Blob(_) => {
				panic!("Should not be Blob for unknown driver prefix");
			}
		}
	}

	#[rstest]
	fn parse_stdin_requires_explicit_extension() {
		let mut cursor = Cursor::new(b"stdin data");
		let registry = make_registry();
		let err = DataSource::parse_with_stdin("-", &registry, &mut cursor).unwrap_err();
		let msg = format!("{:?}", err);
		assert!(
			msg.contains("explicit extension must be provided"),
			"Error message: {}",
			msg
		);
	}

	#[rstest]
	fn parse_stdin_with_mbtiles_prefix_reads_from_reader() {
		let mut cursor = Cursor::new(b"hello from stdin");
		let registry = make_registry();
		let ds = DataSource::parse_with_stdin("mbtiles:-", &registry, &mut cursor).unwrap();
		assert_eq!(ds.extension(), "mbtiles");
		match ds.location() {
			DataLocation::Blob(blob) => {
				assert_eq!(blob.as_str(), "hello from stdin");
			}
			_ => panic!("Expected Blob variant"),
		}
	}

	#[rstest]
	fn from_data_location_derives_extension_or_unknown() {
		let path = std::path::PathBuf::from("/tmp/test.vrt");
		let loc = DataLocation::Path(path.clone());
		let ds = DataSource::from(loc);
		assert_eq!(ds.extension(), "vrt");

		let blob = Blob::from(vec![1u8, 2, 3]);
		let loc = DataLocation::Blob(blob.clone());
		let ds = DataSource::from(loc);
		let expected = ds.location().extension().unwrap_or_else(|_| "unknown".to_string());
		assert_eq!(ds.extension(), expected);
	}

	#[rstest]
	fn resolve_delegates_to_location() {
		let base = DataLocation::from("/base/dir");
		let mut ds = DataSource::from(DataLocation::from("sub/file.mbtiles"));
		ds.resolve(&base).unwrap();
		match ds.location() {
			DataLocation::Path(path) => {
				assert_eq!(path, Path::new("/base/dir/sub/file.mbtiles"));
			}
			_ => panic!("Expected Path variant after resolve"),
		}
	}
}
