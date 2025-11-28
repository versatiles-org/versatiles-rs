//! The `DataSource` struct represents a driver-aware wrapper around a `DataLocation`,
//! allowing for parsing and handling of input specifications that may include
//! driver prefixes, standard input, and extension inference.

use super::data_location::DataLocation;
use anyhow::Result;
use regex::Regex;
use versatiles_derive::context;

#[derive(Debug, Clone, PartialEq)]
/// Represents a parsed input specification which may include a driver prefix (like `mbtiles:`).
/// It can resolve standard input (`-`) into an in-memory Blob and derives the effective extension
/// that determines which reader to pick.
pub struct DataSource {
	container_type: String, // mbtiles / vpl / ...
	name: String,           // name identifier
	location: DataLocation, // URL, filesystem path, or in-memory blob
}

lazy_static::lazy_static! {
	static ref RE_PREFIX: Regex = Regex::new(r#"^\[([\w,]*)\](.*)"#).unwrap();
}

impl DataSource {
	/// Returns the effective extension for this data source.
	///
	/// This is typically the driver prefix if one was specified (e.g., `mbtiles`),
	/// otherwise it is inferred from the underlying location's extension.
	pub fn container_type(&self) -> &str {
		&self.container_type
	}

	pub fn name(&self) -> &str {
		&self.name
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
	/// The input grammar supports optional name and container_type prefixes (e.g., `[osm,mbtiles]`),
	/// standard input (`-`) which is resolved into a Blob (requiring an explicit extension),
	/// and falls back to interpreting the string as a path or URL.
	#[context("parsing data source from input string '{}'", input)]
	pub fn parse(input: &str) -> Result<Self> {
		let mut name: Option<String> = None;
		let mut container_type: Option<String> = None;

		let location_str = if let Some(captures) = RE_PREFIX.captures(input) {
			let prefix = captures.get(1).unwrap().as_str();

			let prefix = prefix.split(',').collect::<Vec<_>>();

			name = prefix.get(0).and_then(|s| (!s.is_empty()).then(|| s.to_string()));
			container_type = prefix.get(1).and_then(|s| (!s.is_empty()).then(|| s.to_string()));

			captures.get(2).unwrap().as_str()
		} else {
			input
		};

		let location = DataLocation::try_from(location_str)?;

		let container_type = match container_type {
			Some(ct) => ct,
			None => location.extension().context("Could not determine container type")?,
		};

		let name = match name {
			Some(n) => n,
			None => location.name().context("Could not determine container name")?,
		};

		Ok(DataSource {
			container_type,
			name,
			location,
		})

		/*
		let stdin = std::io::stdin();
		let mut stdin_lock = stdin.lock();
		Self::parse_with_stdin(s, registry, &mut stdin_lock)
		 */
	}

	/// Resolves the underlying location relative to a base location.
	///
	/// This delegates to the `DataLocation::resolve` method.
	pub fn resolve(&mut self, base: &DataLocation) -> Result<()> {
		self.location.resolve(base)
	}
}

impl TryFrom<&str> for DataSource {
	type Error = anyhow::Error;
	fn try_from(s: &str) -> Result<Self> {
		Self::parse(s)
	}
}

impl TryFrom<&String> for DataSource {
	type Error = anyhow::Error;
	fn try_from(s: &String) -> Result<Self> {
		Self::parse(s)
	}
}

impl TryFrom<DataLocation> for DataSource {
	type Error = anyhow::Error;
	fn try_from(location: DataLocation) -> Result<Self> {
		Ok(DataSource {
			container_type: location.extension()?,
			name: location.name()?,
			location,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use reqwest::Url;
	use rstest::rstest;
	use std::path::PathBuf;

	#[rstest]
	#[case("[,]file.ext", "file", "ext", "file.ext")]
	#[case("[]file.ext", "file", "ext", "file.ext")]
	#[case("file.ext", "file", "ext", "file.ext")]
	#[case("[,type]file.ext", "file", "type", "file.ext")]
	#[case("[name,]file.ext", "name", "ext", "file.ext")]
	#[case("[name,type]file.ext", "name", "type", "file.ext")]
	#[case("[name,type]file", "name", "type", "file")]
	#[case("[name]file.ext", "name", "ext", "file.ext")]
	fn test_parse_with_prefixes(
		#[case] input: &str,
		#[case] expected_name: &str,
		#[case] expected_container: &str,
		#[case] expected_location: &str,
	) {
		let ds = DataSource::parse(input).unwrap();
		assert_eq!(ds.name, expected_name);
		assert_eq!(ds.container_type, expected_container);
		assert_eq!(ds.location.filename().unwrap(), expected_location);
	}

	#[test]
	fn parse_simple_path_uses_path_extension() {
		let ds = DataSource::parse("data/example.mbtiles").unwrap();
		assert_eq!(ds.container_type(), "mbtiles");
		assert_eq!(ds.location().as_path().unwrap().file_name().unwrap(), "example.mbtiles");
	}

	#[test]
	fn parse_url_uses_url_extension() {
		let ds = DataSource::parse("https://example.org/tiles/test.tar").unwrap();
		assert_eq!(ds.container_type(), "tar");
		assert!(ds.location().as_url().unwrap().to_string().ends_with("test.tar"));
	}

	#[test]
	fn from_path() {
		let loc = DataLocation::from(PathBuf::from("/tmp/test.vrt"));
		let ds = DataSource::try_from(loc).unwrap();
		assert_eq!(ds.container_type(), "vrt");
		assert_eq!(ds.name(), "test");
		assert_eq!(ds.location().as_path().unwrap(), "/tmp/test.vrt");
	}

	#[test]
	fn resolve_with_path_base_updates_location() {
		let mut ds = DataSource::parse("rel/tiles/data.mbtiles").unwrap();
		let base = DataLocation::from(PathBuf::from("/data/base"));
		ds.resolve(&base).unwrap();

		let path = ds.location().as_path().unwrap();
		assert_eq!(path, PathBuf::from("/data/base/rel/tiles/data.mbtiles"));
		assert_eq!(ds.container_type(), "mbtiles");
	}

	#[test]
	fn resolve_with_url_base_turns_path_into_url() {
		let mut ds = DataSource::parse("rel/tiles/data.mvt").unwrap();
		let base_url = Url::parse("https://example.org/tiles/").unwrap();
		let base_loc = DataLocation::from(base_url);

		ds.resolve(&base_loc).unwrap();

		let url = ds.location().as_url().unwrap();
		// URL join semantics will normalize the path
		assert_eq!(url.as_str(), "https://example.org/tiles/rel/tiles/data.mvt");
		assert_eq!(ds.container_type(), "mvt");
	}

	#[test]
	fn try_from_str_uses_parse() {
		let s = "data/source.vpl";
		let ds = DataSource::try_from(s).unwrap();

		assert_eq!(ds.container_type(), "vpl");
		assert_eq!(ds.name(), "source");
		assert!(
			ds.location()
				.as_path()
				.unwrap()
				.to_string_lossy()
				.ends_with("data/source.vpl")
		);
	}

	#[test]
	fn try_from_string_ref_uses_parse() {
		let s = String::from("inputs/other.mbtiles");
		let ds = DataSource::try_from(&s).unwrap();

		assert_eq!(ds.container_type(), "mbtiles");
		assert_eq!(ds.name(), "other");
		assert!(
			ds.location()
				.as_path()
				.unwrap()
				.to_string_lossy()
				.ends_with("inputs/other.mbtiles")
		);
	}

	#[test]
	fn into_location_consumes_datasource_and_returns_inner_location() {
		let ds = DataSource::parse("out/result.tar").unwrap();
		let loc = ds.into_location();

		let path = loc.as_path().unwrap();
		assert!(path.to_string_lossy().ends_with("out/result.tar"));
	}
}
