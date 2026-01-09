//! The `DataSource` struct represents a driver-aware wrapper around a `DataLocation`,
//! allowing for parsing and handling of input specifications that may include
//! driver prefixes, standard input, and extension inference.

use super::data_location::DataLocation;
use anyhow::Result;
use regex::Regex;
use versatiles_core::{Blob, json::JsonValue};
use versatiles_derive::context;

#[derive(Debug, Clone, PartialEq)]
/// Represents a parsed input specification which may include a driver prefix (like `mbtiles:`).
/// It can resolve standard input (`-`) into an in-memory Blob and derives the effective extension
/// that determines which reader to pick.
pub struct DataSource {
	container_type: Option<String>, // mbtiles / vpl / ...
	name: Option<String>,           // name identifier
	location: DataLocation,         // URL, filesystem path, or in-memory blob
}

use std::sync::LazyLock;

static RE_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([\w,]*)\](.*)").unwrap());
static RE_POSTFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*)\[([\w,]*)\]$").unwrap());

impl DataSource {
	/// Returns an optional reference to the container type, if specified.
	#[must_use]
	pub fn optional_container_type(&self) -> Option<&str> {
		self.container_type.as_deref()
	}

	/// Returns an optional reference to the name, if specified.
	#[must_use]
	pub fn optional_name(&self) -> Option<&str> {
		self.name.as_deref()
	}

	/// Returns a reference to the container type, or an error if not specified.
	pub fn container_type(&self) -> Result<&str> {
		self
			.container_type
			.as_deref()
			.ok_or(anyhow::anyhow!("Could not determine container type for data source"))
	}

	/// Returns a reference to the name, or an error if not specified.
	pub fn name(&self) -> Result<&str> {
		self
			.name
			.as_deref()
			.ok_or(anyhow::anyhow!("Could not determine name for data source"))
	}

	/// Returns a reference to the underlying `DataLocation`.
	#[must_use]
	pub fn location(&self) -> &DataLocation {
		&self.location
	}

	/// Consumes the `DataSource` and returns the underlying `DataLocation`.
	#[must_use]
	pub fn into_location(self) -> DataLocation {
		self.location
	}

	/// Parses a string specification into a `DataSource`.
	///
	/// The input grammar supports optional name and `container_type` prefixes (e.g., `[osm,mbtiles]`),
	/// standard input (`-`) which is resolved into a Blob (requiring an explicit extension),
	/// and falls back to interpreting the string as a path or URL.
	#[context("parsing data source from input string '{}'", input)]
	pub fn parse(input: &str) -> Result<Self> {
		let mut name: Option<String> = None;
		let mut container_type: Option<String> = None;

		fn extract_name_and_type(text: &str) -> (Option<String>, Option<String>) {
			let mut parts = text.split(',');
			let name = parts.next().and_then(|s| (!s.is_empty()).then(|| s.to_string()));
			let container_type = parts.next().and_then(|s| (!s.is_empty()).then(|| s.to_string()));
			(name, container_type)
		}

		let location = if input.starts_with('{') {
			// JSON blob input
			let json = JsonValue::parse_str(input)?.into_object()?;
			name = json.get("name").map(JsonValue::to_string).transpose()?;
			container_type = json.get("type").map(JsonValue::to_string).transpose()?;

			if let Some(content) = json.get("content") {
				let blob = Blob::from(content.to_string()?);
				DataLocation::from(blob)
			} else {
				DataLocation::from(
					json
						.get("location")
						.ok_or(anyhow::anyhow!("missing `location`"))?
						.to_string()?,
				)
			}
		} else if input.starts_with('[')
			&& let Some(captures) = RE_PREFIX.captures(input)
		{
			// Prefix notation with optional name and container type
			(name, container_type) = extract_name_and_type(captures.get(1).unwrap().as_str());
			DataLocation::try_from(captures.get(2).unwrap().as_str())?
		} else if input.ends_with(']')
			&& let Some(captures) = RE_POSTFIX.captures(input)
		{
			// Postfix notation with optional name and container type
			(name, container_type) = extract_name_and_type(captures.get(2).unwrap().as_str());
			DataLocation::try_from(captures.get(1).unwrap().as_str())?
		} else {
			// No prefixes, just a plain location
			DataLocation::try_from(input)?
		};

		if container_type.is_none() {
			container_type = location.extension().ok();
		}

		if name.is_none() {
			name = location.name().ok();
		}

		Ok(DataSource {
			container_type,
			name,
			location,
		})
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
			container_type: location.extension().ok(),
			name: location.name().ok(),
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
	#[case("(ascii)", "", "", "Blob(len=5)")]
	#[case("(ascii)[,type]", "", "type", "Blob(len=5)")]
	#[case("(ascii)[name,type]", "name", "type", "Blob(len=5)")]
	#[case("(ascii)[name]", "name", "", "Blob(len=5)")]
	#[case("[,]file.ext", "file", "ext", "Path(file.ext)")]
	#[case("[,type](ascii)", "", "type", "Blob(len=5)")]
	#[case("[,type]file.ext", "file", "type", "Path(file.ext)")]
	#[case("[]file.ext", "file", "ext", "Path(file.ext)")]
	#[case("[name,]file.ext", "name", "ext", "Path(file.ext)")]
	#[case("[name,type](ascii)", "name", "type", "Blob(len=5)")]
	#[case("[name,type]file.ext", "name", "type", "Path(file.ext)")]
	#[case("[name,type]file", "name", "type", "Path(file)")]
	#[case("[name](ascii)", "name", "", "Blob(len=5)")]
	#[case("[name]file.ext", "name", "ext", "Path(file.ext)")]
	#[case("[name]http://host/file.ext", "name", "ext", "Url(http://host/file.ext)")]
	#[case("file.ext", "file", "ext", "Path(file.ext)")]
	#[case("file.ext[,]", "file", "ext", "Path(file.ext)")]
	#[case("file.ext[,type]", "file", "type", "Path(file.ext)")]
	#[case("file.ext[]", "file", "ext", "Path(file.ext)")]
	#[case("file.ext[name,]", "name", "ext", "Path(file.ext)")]
	#[case("file.ext[name,type]", "name", "type", "Path(file.ext)")]
	#[case("file.ext[name]", "name", "ext", "Path(file.ext)")]
	#[case("file[name,type]", "name", "type", "Path(file)")]
	#[case("http://host/file.ext", "file", "ext", "Url(http://host/file.ext)")]
	#[case("http://host/file.ext[name]", "name", "ext", "Url(http://host/file.ext)")]
	#[case(r#"{"location":"c"}"#, "c", "", "Path(c)")]
	#[case(r#"{"name":"a","location":"c"}"#, "a", "", "Path(c)")]
	#[case(r#"{"name":"a","type":"b","content":"c"}"#, "a", "b", "Blob(len=1)")]
	#[case(r#"{"name":"a","type":"b","location":"c"}"#, "a", "b", "Path(c)")]
	#[case(r#"{"name":"a","type":"b","location":"http://c"}"#, "a", "b", "Url(http://c/)")]
	#[case(r#"{"type":"b","location":"c"}"#, "c", "b", "Path(c)")]
	fn parse_with_prefixes(
		#[case] input: &str,
		#[case] exp_name: &str,
		#[case] exp_container: &str,
		#[case] exp_location: &str,
	) {
		let ds = DataSource::parse(input).unwrap();
		assert_eq!(
			ds.optional_name(),
			(!exp_name.is_empty()).then_some(exp_name),
			"name for '{input}'"
		);
		assert_eq!(
			ds.optional_container_type(),
			(!exp_container.is_empty()).then_some(exp_container),
			"container_type for '{input}'"
		);
		assert_eq!(format!("{:?}", ds.location), exp_location, "location for '{input}'");
	}

	#[test]
	fn parse_simple_path_uses_path_extension() {
		let ds = DataSource::parse("data/example.mbtiles").unwrap();
		assert_eq!(ds.container_type().unwrap(), "mbtiles");
		assert_eq!(ds.location().as_path().unwrap().file_name().unwrap(), "example.mbtiles");
	}

	#[test]
	fn parse_url_uses_url_extension() {
		let ds = DataSource::parse("https://example.org/tiles/test.tar").unwrap();
		assert_eq!(ds.container_type().unwrap(), "tar");
		assert!(ds.location().as_url().unwrap().to_string().ends_with("test.tar"));
	}

	#[test]
	fn from_path() {
		let loc = DataLocation::from(PathBuf::from("/tmp/test.vrt"));
		let ds = DataSource::try_from(loc).unwrap();
		assert_eq!(ds.container_type().unwrap(), "vrt");
		assert_eq!(ds.name().unwrap(), "test");
		assert_eq!(ds.location().as_path().unwrap(), "/tmp/test.vrt");
	}

	#[test]
	fn resolve_with_path_base_updates_location() {
		let mut ds = DataSource::parse("rel/tiles/data.mbtiles").unwrap();
		let base = DataLocation::from(PathBuf::from("/data/base"));
		ds.resolve(&base).unwrap();

		let path = ds.location().as_path().unwrap();
		assert_eq!(path, PathBuf::from("/data/base/rel/tiles/data.mbtiles"));
		assert_eq!(ds.container_type().unwrap(), "mbtiles");
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
		assert_eq!(ds.container_type().unwrap(), "mvt");
	}

	#[test]
	fn try_from_str_uses_parse() {
		let s = "data/source.vpl";
		let ds = DataSource::try_from(s).unwrap();

		assert_eq!(ds.container_type().unwrap(), "vpl");
		assert_eq!(ds.name().unwrap(), "source");
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

		assert_eq!(ds.container_type().unwrap(), "mbtiles");
		assert_eq!(ds.name().unwrap(), "other");
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
