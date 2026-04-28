//! CSV [`FeatureSource`] adapter.
//!
//! Reads a CSV file with explicit longitude/latitude columns. Each row
//! becomes a [`GeoFeature`] with a `Point` geometry. Remaining columns
//! become string properties. An optional `id_column` is exposed as the
//! feature's `id`.
//!
//! v1 only supports the lon/lat path — WKT geometry columns are out of
//! scope. The `lon_column` and `lat_column` fields are both required.

use super::FeatureSource;
use crate::geo::{GeoFeature, GeoProperties, GeoValue};
use anyhow::{Context, Result, bail};
use futures::stream::{self, BoxStream, StreamExt};
use geo_types::{Geometry, Point};
use std::{
	fs::File,
	io::BufReader,
	path::{Path, PathBuf},
};

/// Reads point features from a CSV file using explicit lon/lat columns.
#[derive(Debug, Clone)]
pub struct CsvSource {
	path: PathBuf,
	name: String,
	lon_column: String,
	lat_column: String,
	id_column: Option<String>,
	delimiter: u8,
	has_header: bool,
}

/// Builder for [`CsvSource`]. `path`, `lon_column`, and `lat_column` are required.
#[derive(Debug, Clone)]
pub struct CsvSourceBuilder {
	path: PathBuf,
	lon_column: String,
	lat_column: String,
	id_column: Option<String>,
	delimiter: u8,
	has_header: bool,
}

impl CsvSourceBuilder {
	/// Construct a builder pointing at `path` with the required lon/lat columns.
	#[must_use]
	pub fn new(path: impl AsRef<Path>, lon_column: impl Into<String>, lat_column: impl Into<String>) -> Self {
		Self {
			path: path.as_ref().to_path_buf(),
			lon_column: lon_column.into(),
			lat_column: lat_column.into(),
			id_column: None,
			delimiter: b',',
			has_header: true,
		}
	}

	/// Override the column whose value becomes the feature's `id` (numeric or string).
	#[must_use]
	pub fn id_column(mut self, name: impl Into<String>) -> Self {
		self.id_column = Some(name.into());
		self
	}

	/// Override the field delimiter (default `,`).
	#[must_use]
	pub fn delimiter(mut self, delimiter: u8) -> Self {
		self.delimiter = delimiter;
		self
	}

	/// Whether the first row contains column names (default `true`). Header-less
	/// CSVs aren't supported in v1; `false` is rejected at [`build`](Self::build).
	#[must_use]
	pub fn has_header(mut self, has_header: bool) -> Self {
		self.has_header = has_header;
		self
	}

	/// Finalize the builder.
	pub fn build(self) -> Result<CsvSource> {
		if !self.has_header {
			bail!("CSV inputs without a header row are not supported in v1");
		}
		let name = self
			.path
			.file_stem()
			.and_then(|s| s.to_str())
			.unwrap_or("features")
			.to_string();
		Ok(CsvSource {
			path: self.path,
			name,
			lon_column: self.lon_column,
			lat_column: self.lat_column,
			id_column: self.id_column,
			delimiter: self.delimiter,
			has_header: self.has_header,
		})
	}
}

impl FeatureSource for CsvSource {
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>> {
		let file = File::open(&self.path).with_context(|| format!("opening CSV file {}", self.path.display()))?;
		let reader = BufReader::new(file);
		let mut csv_reader = csv::ReaderBuilder::new()
			.delimiter(self.delimiter)
			.has_headers(self.has_header)
			.from_reader(reader);

		let headers = csv_reader
			.headers()
			.with_context(|| format!("reading CSV header from {}", self.path.display()))?
			.clone();

		let lon_idx = column_index(&headers, &self.lon_column)?;
		let lat_idx = column_index(&headers, &self.lat_column)?;
		let id_idx = self
			.id_column
			.as_deref()
			.map(|name| column_index(&headers, name))
			.transpose()?;

		let header_names: Vec<String> = headers.iter().map(str::to_string).collect();
		let path_for_errors = self.path.clone();

		let mut features: Vec<Result<GeoFeature>> = Vec::new();
		for (row_idx, record) in csv_reader.records().enumerate() {
			let record = match record {
				Ok(r) => r,
				Err(e) => {
					features.push(Err(anyhow::Error::new(e).context(format!(
						"reading CSV record {} of {}",
						row_idx + 1,
						path_for_errors.display()
					))));
					continue;
				}
			};
			match build_feature(&record, lon_idx, lat_idx, id_idx, &header_names) {
				Ok(feature) => features.push(Ok(feature)),
				Err(e) => features.push(Err(e.context(format!(
					"row {} in {}",
					row_idx + 1,
					path_for_errors.display()
				)))),
			}
		}

		Ok(stream::iter(features).boxed())
	}

	fn name(&self) -> &str {
		&self.name
	}
}

fn column_index(headers: &csv::StringRecord, name: &str) -> Result<usize> {
	headers
		.iter()
		.position(|h| h == name)
		.ok_or_else(|| anyhow::anyhow!("CSV header is missing required column '{name}'"))
}

fn build_feature(
	record: &csv::StringRecord,
	lon_idx: usize,
	lat_idx: usize,
	id_idx: Option<usize>,
	header_names: &[String],
) -> Result<GeoFeature> {
	let lon_str = record
		.get(lon_idx)
		.ok_or_else(|| anyhow::anyhow!("missing lon column value"))?;
	let lat_str = record
		.get(lat_idx)
		.ok_or_else(|| anyhow::anyhow!("missing lat column value"))?;
	let lon: f64 = lon_str
		.parse()
		.with_context(|| format!("parsing lon value '{lon_str}'"))?;
	let lat: f64 = lat_str
		.parse()
		.with_context(|| format!("parsing lat value '{lat_str}'"))?;

	let mut properties = GeoProperties::new();
	for (idx, name) in header_names.iter().enumerate() {
		// Skip the geometry columns; their values are encoded in the geometry.
		if idx == lon_idx || idx == lat_idx {
			continue;
		}
		// id_column is captured separately; don't duplicate it as a property.
		if Some(idx) == id_idx {
			continue;
		}
		let value = record.get(idx).unwrap_or("");
		properties.insert(name.clone(), GeoValue::from(value));
	}

	let id = id_idx.and_then(|i| record.get(i)).map(parse_id);

	Ok(GeoFeature {
		id,
		geometry: Geometry::Point(Point::new(lon, lat)),
		properties,
	})
}

/// Parse an id-column value as a `u64` if it parses cleanly; otherwise as a string.
fn parse_id(raw: &str) -> GeoValue {
	if let Ok(n) = raw.parse::<u64>() {
		return GeoValue::from(n);
	}
	GeoValue::from(raw)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ext::type_name;
	use futures::StreamExt;

	const FIXTURE: &str = "../testdata/quakes.csv";

	#[tokio::test]
	async fn loads_fixture_features() -> Result<()> {
		let source = CsvSourceBuilder::new(FIXTURE, "longitude", "latitude")
			.id_column("event_id")
			.build()?;
		assert_eq!(source.name(), "quakes");

		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		assert_eq!(features.len(), 3);
		assert_eq!(type_name(&features[0].geometry), "Point");
		assert_eq!(features[0].id, Some(GeoValue::from(1u64)));
		assert_eq!(features[0].properties.get("magnitude"), Some(&GeoValue::from("4.2")));
		// Geometry columns are NOT in the property map.
		assert!(features[0].properties.get("longitude").is_none());
		assert!(features[0].properties.get("latitude").is_none());
		// id column is NOT in the property map either.
		assert!(features[0].properties.get("event_id").is_none());
		Ok(())
	}

	#[tokio::test]
	async fn missing_lon_column_errors() -> Result<()> {
		let source = CsvSourceBuilder::new(FIXTURE, "lon_missing", "latitude").build()?;
		let result = source.load();
		assert!(result.is_err());
		let err_str = format!("{:#}", result.err().unwrap());
		assert!(err_str.contains("lon_missing"), "{err_str}");
		Ok(())
	}

	#[tokio::test]
	async fn missing_id_column_errors() -> Result<()> {
		let source = CsvSourceBuilder::new(FIXTURE, "longitude", "latitude")
			.id_column("does_not_exist")
			.build()?;
		let result = source.load();
		assert!(result.is_err());
		let err_str = format!("{:#}", result.err().unwrap());
		assert!(err_str.contains("does_not_exist"), "{err_str}");
		Ok(())
	}

	#[test]
	fn header_less_csv_rejected() {
		let result = CsvSourceBuilder::new(FIXTURE, "longitude", "latitude")
			.has_header(false)
			.build();
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn unparseable_lon_emits_row_error() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let path = dir.path().join("bad.csv");
		std::fs::write(&path, "lon,lat,name\n1.0,2.0,A\nnot_a_number,3.0,B\n")?;

		let source = CsvSourceBuilder::new(&path, "lon", "lat").build()?;
		let mut stream = source.load()?;
		let mut ok = 0;
		let mut err = 0;
		while let Some(item) = stream.next().await {
			match item {
				Ok(_) => ok += 1,
				Err(_) => err += 1,
			}
		}
		assert_eq!(ok, 1);
		assert_eq!(err, 1);
		Ok(())
	}

	#[test]
	fn parse_id_prefers_numeric() {
		assert_eq!(parse_id("42"), GeoValue::from(42u64));
		assert_eq!(parse_id("not-a-number"), GeoValue::from("not-a-number"));
	}
}
