//! The `DataSource` struct represents a driver-aware wrapper around a `DataLocation`,
//! allowing for parsing and handling of input specifications that may include
//! driver prefixes, standard input, and extension inference.

use anyhow::Result;
use regex::Regex;
use versatiles_core::{Blob, json::JsonValue};
use versatiles_derive::context;

use super::data_location::DataLocation;

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

static RE_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([\w,]*)\](.*)").expect("valid regex literal"));
static RE_POSTFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(.*)\[([\w,]*)\]$").expect("valid regex literal"));

/// Whether a trailing `[…]` is the name/type postfix rather than something else.
///
/// The notation is `location[name,type]`, and `[\w,]*` also matches a VPL array — every grid
/// pipeline ends in one, `bbox=[13,52,14,53]`. Read as a postfix, that swallows the parameter
/// and leaves a location ending in `bbox=`, which is how a pipeline turned into a container
/// type of `52`. Two things tell them apart: a postfix follows a location, never an `=`, and it
/// holds at most a name and a type.
fn is_name_and_type(location: &str, postfix: &str) -> bool {
	!location.trim_end().ends_with('=') && postfix.split(',').count() <= 2
}

/// Whether some text reads as a VPL pipeline rather than a filename.
///
/// Used only to explain a failure — a pipeline has to be typed as `[,vpl](…)` or live in a
/// `.vpl` file, and the error for forgetting that used to be about a missing path. So this is
/// allowed to be approximate: an operation name, then a space, then either a parameter or
/// another stage.
#[must_use]
pub(crate) fn looks_like_vpl(text: &str) -> bool {
	text.starts_with(|c: char| c.is_ascii_alphabetic())
		&& text.contains(char::is_whitespace)
		&& (text.contains('=') || text.contains('|'))
}

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
				DataLocation::try_from(
					json
						.get("location")
						.ok_or(anyhow::anyhow!("missing `location`"))?
						.to_string()?,
				)?
			}
		} else if input.starts_with('[')
			&& let Some(captures) = RE_PREFIX.captures(input)
		{
			// Prefix notation with optional name and container type
			(name, container_type) = extract_name_and_type(captures.get(1).expect("regex has two groups").as_str());
			DataLocation::try_from(captures.get(2).expect("regex has two groups").as_str())?
		} else if input.ends_with(']')
			&& let Some(captures) = RE_POSTFIX.captures(input)
			&& is_name_and_type(
				captures.get(1).expect("regex has two groups").as_str(),
				captures.get(2).expect("regex has two groups").as_str(),
			) {
			// Postfix notation with optional name and container type
			(name, container_type) = extract_name_and_type(captures.get(2).expect("regex has two groups").as_str());
			DataLocation::try_from(captures.get(1).expect("regex has two groups").as_str())?
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
	/// Every grid pipeline ends in a bbox, and `[\w,]*` matches one — read as the name/type
	/// postfix, it eats the parameter and leaves a location ending in `bbox=`.
	#[test]
	fn a_vpl_array_is_not_a_name_and_type_postfix() {
		for input in [
			"from_h3 resolution=8 bbox=[13,52,14,53]",
			"from_grid epsg=3035 size=1000 bbox=[5,47,15,55]",
			"filter layer=[places]",
		] {
			let source = DataSource::parse(input).expect("parses as a plain location");
			assert_eq!(
				source.location().to_string(),
				input,
				"the location lost part of itself: {source:?}"
			);
		}
	}

	#[test]
	fn a_real_postfix_still_names_the_source() {
		let source = DataSource::parse("world.mbtiles[osm,mbtiles]").unwrap();
		assert_eq!(source.optional_name(), Some("osm"));
		assert_eq!(source.optional_container_type(), Some("mbtiles"));

		let source = DataSource::parse("pipeline.txt[osm,vpl]").unwrap();
		assert_eq!(source.optional_name(), Some("osm"));
		assert_eq!(source.optional_container_type(), Some("vpl"));
	}

	#[test]
	fn more_than_a_name_and_a_type_is_not_a_postfix() {
		let source = DataSource::parse("thing[a,b,c]").unwrap();
		assert_eq!(source.location().to_string(), "thing[a,b,c]");
	}

	#[test]
	fn vpl_text_is_told_apart_from_a_filename() {
		for text in [
			"from_debug format=png",
			"from_h3 resolution=8 bbox=[13,52,14,53]",
			"from_container filename=world.versatiles | filter level_min=5",
		] {
			assert!(looks_like_vpl(text), "{text}");
		}

		for text in [
			"berlin.mbtiles",
			"/data/tiles/world.versatiles",
			"a file with spaces.mbtiles",
			"https://example.org/tiles.pmtiles",
			"C:\\Users\\me\\tiles.mbtiles",
		] {
			assert!(!looks_like_vpl(text), "{text}");
		}
	}

	use std::path::PathBuf;

	use reqwest::Url;
	use rstest::rstest;

	use super::*;

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
