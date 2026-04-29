//! GeoJSON [`FeatureSource`] adapter.
//!
//! Supports two on-disk shapes:
//!
//! - [`Format::FeatureCollection`] — a single JSON document containing a
//!   GeoJSON `FeatureCollection`. Read in full with
//!   [`crate::geojson::read_geojson`].
//! - [`Format::LineDelimited`] — newline-delimited GeoJSON (NDGeoJSON /
//!   GeoJSONL): one feature per line. Read line-by-line via
//!   [`crate::geojson::read_ndgeojson_iter`].

use super::FeatureSource;
use crate::{
	geo::GeoFeature,
	geojson::{read_geojson, read_ndgeojson_iter},
};
use anyhow::{Context, Result};
use futures::stream::{self, BoxStream, StreamExt};
use std::{
	fs::File,
	io::BufReader,
	path::{Path, PathBuf},
};

/// On-disk shape of a GeoJSON file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
	/// A single JSON document holding a `FeatureCollection`.
	FeatureCollection,
	/// One GeoJSON feature per line (NDGeoJSON / GeoJSONL).
	LineDelimited,
}

/// Reads features from a GeoJSON file.
#[derive(Debug, Clone)]
pub struct GeoJsonSource {
	path: PathBuf,
	name: String,
	format: Format,
}

impl GeoJsonSource {
	/// Construct a new source for a `FeatureCollection` file.
	///
	/// `name()` returns the filename stem of `path`.
	#[must_use]
	pub fn new(path: impl AsRef<Path>) -> Self {
		Self::with_format(path, Format::FeatureCollection)
	}

	/// Construct a new source for a newline-delimited GeoJSON file
	/// (one feature per line).
	#[must_use]
	pub fn new_line_delimited(path: impl AsRef<Path>) -> Self {
		Self::with_format(path, Format::LineDelimited)
	}

	#[must_use]
	pub fn with_format(path: impl AsRef<Path>, format: Format) -> Self {
		let path = path.as_ref().to_path_buf();
		let name = path
			.file_stem()
			.and_then(|s| s.to_str())
			.unwrap_or("features")
			.to_string();
		Self { path, name, format }
	}
}

impl FeatureSource for GeoJsonSource {
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>> {
		let file = File::open(&self.path).with_context(|| format!("opening GeoJSON file {}", self.path.display()))?;
		match self.format {
			Format::FeatureCollection => {
				let collection = read_geojson(BufReader::new(file))
					.with_context(|| format!("parsing GeoJSON file {}", self.path.display()))?;
				Ok(stream::iter(collection.features.into_iter().map(Ok)).boxed())
			}
			Format::LineDelimited => {
				let path = self.path.clone();
				let features: Vec<Result<GeoFeature>> = read_ndgeojson_iter(BufReader::new(file))
					.map(move |item| item.with_context(|| format!("parsing NDGeoJSON file {}", path.display())))
					.collect();
				Ok(stream::iter(features).boxed())
			}
		}
	}

	fn name(&self) -> &str {
		&self.name
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ext::type_name;
	use futures::StreamExt;

	const FIXTURE: &str = "../testdata/places.geojson";

	#[tokio::test]
	async fn loads_fixture_features() -> Result<()> {
		let source = GeoJsonSource::new(FIXTURE);
		assert_eq!(source.name(), "places");

		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		assert_eq!(features.len(), 4, "fixture has 4 features");
		assert_eq!(type_name(&features[0].geometry), "Point");
		assert_eq!(type_name(&features[1].geometry), "LineString");
		assert_eq!(type_name(&features[2].geometry), "Polygon");
		assert_eq!(type_name(&features[3].geometry), "MultiPolygon");
		Ok(())
	}

	#[test]
	fn name_falls_back_for_extensionless_path() {
		let s = GeoJsonSource::new("foo");
		assert_eq!(s.name(), "foo");
	}

	#[test]
	fn missing_file_errors() {
		let s = GeoJsonSource::new("/nonexistent/does-not-exist.geojson");
		assert!(s.load().is_err());
	}

	#[tokio::test]
	async fn loads_line_delimited_fixture() -> Result<()> {
		let source = GeoJsonSource::new_line_delimited("../testdata/places.geojsonl");
		assert_eq!(source.name(), "places");

		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		assert_eq!(features.len(), 4, "fixture has 4 features");
		assert_eq!(type_name(&features[0].geometry), "Point");
		assert_eq!(type_name(&features[1].geometry), "LineString");
		assert_eq!(type_name(&features[2].geometry), "Polygon");
		assert_eq!(type_name(&features[3].geometry), "MultiPolygon");
		Ok(())
	}

	#[tokio::test]
	async fn line_delimited_skips_blank_lines() -> Result<()> {
		// Line-delimited fixture with a blank line between features — should
		// yield 2 features, not error.
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("blanks.geojsonl");
		std::fs::write(
			&path,
			"{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[0,0]},\"properties\":{}}\n\
			 \n\
			 {\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[1,1]},\"properties\":{}}\n",
		)?;

		let source = GeoJsonSource::new_line_delimited(&path);
		let mut stream = source.load()?;
		let mut count = 0;
		while let Some(item) = stream.next().await {
			item?;
			count += 1;
		}
		assert_eq!(count, 2);
		Ok(())
	}

	#[tokio::test]
	async fn line_delimited_errors_on_bad_line() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("bad.geojsonl");
		std::fs::write(
			&path,
			"{\"type\":\"Feature\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[0,0]},\"properties\":{}}\n\
			 {not valid json}\n",
		)?;

		let source = GeoJsonSource::new_line_delimited(&path);
		let mut stream = source.load()?;
		let mut had_error = false;
		while let Some(item) = stream.next().await {
			if item.is_err() {
				had_error = true;
			}
		}
		assert!(had_error, "malformed line should surface as a stream error");
		Ok(())
	}
}
