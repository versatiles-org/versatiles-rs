//! GeoJSON [`FeatureSource`] adapter.
//!
//! Wraps the existing [`crate::geojson::read_geojson`] parser. v1 reads the
//! whole file into memory and emits the features one by one as a stream.

use super::FeatureSource;
use crate::{geo::GeoFeature, geojson::read_geojson};
use anyhow::{Context, Result};
use futures::stream::{self, BoxStream, StreamExt};
use std::{
	fs::File,
	io::BufReader,
	path::{Path, PathBuf},
};

/// Reads features from a GeoJSON `FeatureCollection` file.
#[derive(Debug, Clone)]
pub struct GeoJsonSource {
	path: PathBuf,
	name: String,
}

impl GeoJsonSource {
	/// Construct a new source pointing at the given GeoJSON file.
	///
	/// `name()` returns the filename stem of `path`.
	#[must_use]
	pub fn new(path: impl AsRef<Path>) -> Self {
		let path = path.as_ref().to_path_buf();
		let name = path
			.file_stem()
			.and_then(|s| s.to_str())
			.unwrap_or("features")
			.to_string();
		Self { path, name }
	}
}

impl FeatureSource for GeoJsonSource {
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>> {
		let file = File::open(&self.path).with_context(|| format!("opening GeoJSON file {}", self.path.display()))?;
		let collection =
			read_geojson(BufReader::new(file)).with_context(|| format!("parsing GeoJSON file {}", self.path.display()))?;
		Ok(stream::iter(collection.features.into_iter().map(Ok)).boxed())
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
}
