//! Shapefile [`FeatureSource`] adapter.
//!
//! Reads `.shp` (geometry), `.shx` (index, optional but expected), `.dbf`
//! (attributes), and `.prj` (CRS, when present). Geometry is converted to
//! `geo_types::Geometry<f64>` via the `shapefile` crate's built-in interop.
//! DBF attributes are mapped to [`GeoProperties`] using the
//! [`dbase`] field types.
//!
//! Encoding: the underlying `dbase` reader uses [`UnicodeLossy`]
//! by default — non-UTF-8 bytes are replaced with `U+FFFD`. We log a
//! single `log::warn!` per source if any decoded value contains a
//! replacement character.
//!
//! Projection: only WGS84 input is supported. If a `.prj` file is present
//! and is not WGS84, [`load`](FeatureSource::load) bails.

use super::{FeatureSource, ProgressCallback, ProgressReader};
use crate::geo::{GeoFeature, GeoProperties, GeoValue};
use anyhow::{Context, Result, anyhow, bail};
use futures::stream::{self, BoxStream, StreamExt};
use geo_types::Geometry;
use shapefile::Shape;
use shapefile::dbase::{self, FieldValue};
use std::{
	fs,
	path::{Path, PathBuf},
};

/// Reads features from an Esri Shapefile (`.shp` + `.dbf`).
#[derive(Clone)]
pub struct ShapefileSource {
	path: PathBuf,
	name: String,
	progress: Option<ProgressCallback>,
}

impl std::fmt::Debug for ShapefileSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ShapefileSource")
			.field("path", &self.path)
			.field("name", &self.name)
			.field("progress", &self.progress.as_ref().map(|_| "<callback>"))
			.finish()
	}
}

impl ShapefileSource {
	/// Construct a new source pointing at the given `.shp` file.
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
		Self {
			path,
			name,
			progress: None,
		}
	}

	/// Attach a [`ProgressCallback`] reporting bytes consumed from the `.shp`
	/// and `.dbf` sidecars. The smaller `.shx` index is not tracked.
	#[must_use]
	pub fn with_progress(mut self, callback: ProgressCallback) -> Self {
		self.progress = Some(callback);
		self
	}

	/// Read the optional `.prj` sibling file. If present and not WGS84,
	/// returns an error.
	fn check_projection(&self) -> Result<()> {
		let prj_path = self.path.with_extension("prj");
		if !prj_path.exists() {
			return Ok(());
		}
		let contents =
			fs::read_to_string(&prj_path).with_context(|| format!("reading projection file {}", prj_path.display()))?;
		if is_wgs84_prj(&contents) {
			Ok(())
		} else {
			bail!(
				"shapefile {} has a non-WGS84 projection (.prj); reprojection is not supported in v1",
				self.path.display()
			)
		}
	}
}

impl FeatureSource for ShapefileSource {
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>> {
		self.check_projection()?;

		// Open .shp + .dbf manually so we can wrap each in a ProgressReader.
		// (.shx is small and optional — keep the original from_path lookup
		// path for it via with_shx if it exists.)
		let shp_file =
			fs::File::open(&self.path).with_context(|| format!("opening shapefile {}", self.path.display()))?;
		let shp_reader = std::io::BufReader::new(ProgressReader::maybe(shp_file, self.progress.clone()));
		let shx_path = self.path.with_extension("shx");
		let shape_reader = if shx_path.exists() {
			let shx_source = std::io::BufReader::new(
				fs::File::open(&shx_path).with_context(|| format!("opening shx {}", shx_path.display()))?,
			);
			shapefile::ShapeReader::with_shx(shp_reader, shx_source)
				.with_context(|| format!("reading shapefile {}", self.path.display()))?
		} else {
			shapefile::ShapeReader::new(shp_reader)
				.with_context(|| format!("reading shapefile {}", self.path.display()))?
		};

		// Build the dbase reader with `UnicodeLossy` so non-UTF-8 bytes become
		// U+FFFD instead of aborting the whole load — matches the lossy
		// behavior dbase had by default before 0.7.
		let dbf_path = self.path.with_extension("dbf");
		let dbf_file = fs::File::open(&dbf_path).with_context(|| format!("opening dbf {}", dbf_path.display()))?;
		let dbf_buffered = std::io::BufReader::new(ProgressReader::maybe(dbf_file, self.progress.clone()));
		let dbase_reader = dbase::ReaderBuilder::new(dbf_buffered)
			.with_encoding(dbase::encoding::UnicodeLossy)
			.build()
			.with_context(|| format!("reading dbf {}", dbf_path.display()))?;
		let mut reader = shapefile::Reader::new(shape_reader, dbase_reader);

		let mut features: Vec<Result<GeoFeature>> = Vec::new();
		let mut had_lossy = false;

		for entry in reader.iter_shapes_and_records() {
			let (shape, record) = entry.with_context(|| format!("reading shapefile {}", self.path.display()))?;
			let geometry = match shape_to_geometry(shape) {
				Ok(g) => g,
				Err(e) => {
					features.push(Err(e));
					continue;
				}
			};
			let properties = record_to_properties(&record, &mut had_lossy);
			features.push(Ok(GeoFeature {
				id: None,
				geometry,
				properties,
			}));
		}

		if had_lossy {
			log::warn!(
				"shapefile {} contains non-UTF-8 DBF text bytes; affected fields use the Unicode replacement character",
				self.path.display()
			);
		}

		Ok(stream::iter(features).boxed())
	}

	fn name(&self) -> &str {
		&self.name
	}
}

/// Convert a `shapefile::Shape` into a `geo_types::Geometry<f64>`.
///
/// Skips Z/M variants (3D / measured shapes) — v1 is 2D-only — and the
/// `Multipatch` variant (no useful 2D mapping).
fn shape_to_geometry(shape: Shape) -> Result<Geometry<f64>> {
	use shapefile::Shape::{
		Multipatch, Multipoint, MultipointM, MultipointZ, NullShape, Point, PointM, PointZ, Polygon, PolygonM, PolygonZ,
		Polyline, PolylineM, PolylineZ,
	};
	let geometry: Geometry<f64> = match shape {
		NullShape => bail!("shapefile contains a NullShape record"),
		Point(p) => Geometry::try_from(Point(p)).map_err(|e| anyhow!("converting Point: {e:?}"))?,
		Polyline(p) => Geometry::try_from(Polyline(p)).map_err(|e| anyhow!("converting Polyline: {e:?}"))?,
		Polygon(p) => Geometry::try_from(Polygon(p)).map_err(|e| anyhow!("converting Polygon: {e:?}"))?,
		Multipoint(mp) => Geometry::try_from(Multipoint(mp)).map_err(|e| anyhow!("converting Multipoint: {e:?}"))?,
		// Z and M variants drop the Z/M and use the 2D coordinates.
		PointM(_) | PointZ(_) | PolylineM(_) | PolylineZ(_) | PolygonM(_) | PolygonZ(_) | MultipointM(_)
		| MultipointZ(_) => {
			bail!("shapefile contains a 3D or measured shape variant; v1 only supports 2D")
		}
		Multipatch(_) => bail!("shapefile contains a Multipatch shape; not supported in v1"),
	};
	Ok(geometry)
}

/// Convert a DBF record into [`GeoProperties`]. Sets `*had_lossy` to `true`
/// whenever a Character/Memo string contains the Unicode replacement character.
fn record_to_properties(record: &dbase::Record, had_lossy: &mut bool) -> GeoProperties {
	let mut props = GeoProperties::new();
	let map: &std::collections::HashMap<String, FieldValue> = record.as_ref();
	for (name, value) in map {
		if let Some(geo_value) = field_to_geo_value(value, had_lossy) {
			props.insert(name.clone(), geo_value);
		}
	}
	props
}

fn field_to_geo_value(value: &FieldValue, had_lossy: &mut bool) -> Option<GeoValue> {
	match value {
		FieldValue::Character(opt) => opt.as_ref().map(|s| {
			if s.contains('\u{FFFD}') {
				*had_lossy = true;
			}
			GeoValue::from(s.as_str())
		}),
		FieldValue::Numeric(opt) => opt.map(GeoValue::from),
		FieldValue::Logical(opt) => opt.map(GeoValue::from),
		FieldValue::Float(opt) => opt.map(|f| GeoValue::from(f64::from(f))),
		FieldValue::Integer(i) => Some(GeoValue::from(i64::from(*i))),
		FieldValue::Currency(c) => Some(GeoValue::from(*c)),
		FieldValue::Double(d) => Some(GeoValue::from(*d)),
		FieldValue::Memo(s) => {
			if s.contains('\u{FFFD}') {
				*had_lossy = true;
			}
			Some(GeoValue::from(s.as_str()))
		}
		FieldValue::Date(opt) => opt.as_ref().map(|d| GeoValue::from(d.to_string())),
		FieldValue::DateTime(dt) => Some(GeoValue::from(format!("{dt:?}"))),
	}
}

/// Best-effort detection: a `.prj` file is considered WGS84 if it mentions
/// the WGS84 datum (case-insensitive). This is intentionally lax to admit
/// the dozens of WGS84 PRJ string variants seen in practice without
/// dragging in a full WKT parser for v1.
fn is_wgs84_prj(prj: &str) -> bool {
	let lower = prj.to_ascii_lowercase();
	lower.contains("wgs_1984")
		|| lower.contains("wgs 1984")
		|| lower.contains("wgs84")
		|| lower.contains("epsg\",\"4326")
		|| lower.contains("epsg::4326")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ext::type_name;
	use futures::StreamExt;

	const FIXTURE: &str = "../testdata/admin.shp";

	#[tokio::test]
	async fn loads_fixture_features() -> Result<()> {
		let source = ShapefileSource::new(FIXTURE);
		assert_eq!(source.name(), "admin");

		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		assert_eq!(features.len(), 2);
		// shapefile's Polygon → geo-types is `MultiPolygon` (a shapefile Polygon can
		// hold multiple disjoint rings, which only round-trip cleanly as MultiPolygon).
		assert_eq!(type_name(&features[0].geometry), "MultiPolygon");
		assert_eq!(features[0].properties.get("name"), Some(&GeoValue::from("Berlin")));
		assert_eq!(features[0].properties.get("pop_k"), Some(&GeoValue::from(3700.0)));
		assert_eq!(features[1].properties.get("name"), Some(&GeoValue::from("Brandenburg")));
		assert_eq!(features[1].properties.get("pop_k"), Some(&GeoValue::from(2540.0)));
		Ok(())
	}

	#[test]
	fn missing_file_errors() {
		let s = ShapefileSource::new("/nonexistent/does-not-exist.shp");
		assert!(s.load().is_err());
	}

	#[test]
	fn is_wgs84_prj_detects_common_variants() {
		assert!(is_wgs84_prj(r#"GEOGCS["WGS_1984",DATUM["D_WGS_1984"]]"#));
		assert!(is_wgs84_prj(r#"GEOGCS["WGS84",DATUM["WGS84"]]"#));
		assert!(is_wgs84_prj(r#"AUTHORITY["EPSG","4326"]"#));
		assert!(!is_wgs84_prj(r#"GEOGCS["NAD27",DATUM["D_North_American_1927"]]"#));
	}

	#[tokio::test]
	async fn lossy_dbf_decoding_warns_and_continues() -> Result<()> {
		// Build a fixture with a non-UTF-8 byte in a Character field, in a
		// tempdir. This exercises the lossy path without committing a binary
		// fixture for the corner case.
		let dir = tempfile::tempdir()?;
		let shp_path = dir.path().join("bad_utf8.shp");

		write_lossy_fixture(&shp_path)?;

		let source = ShapefileSource::new(&shp_path);
		let mut stream = source.load()?;
		let mut features = Vec::new();
		while let Some(item) = stream.next().await {
			features.push(item?);
		}

		assert_eq!(features.len(), 1);
		// The Character field had a non-UTF-8 byte → decoded with replacement.
		let name = features[0].properties.get("name").expect("name field present");
		match name {
			GeoValue::String(s) => assert!(s.contains('\u{FFFD}'), "expected replacement char in {s:?}"),
			other => panic!("expected String, got {other:?}"),
		}
		Ok(())
	}

	/// Regenerate the committed `testdata/admin.{shp,shx,dbf}` fixture.
	/// Run with `cargo test -p versatiles_geometry -- --ignored regenerate_admin_fixture`
	/// after intentionally changing the fixture shape.
	#[test]
	#[ignore = "regenerates a committed binary fixture; run only when the fixture intentionally changes"]
	fn regenerate_admin_fixture() -> Result<()> {
		use shapefile::dbase::{FieldName, TableWriterBuilder};
		use shapefile::{Polygon, PolygonRing, Writer, record::Point};

		let shp_path = std::path::PathBuf::from(FIXTURE);
		let dbf_path = shp_path.with_extension("dbf");
		// Remove the .shx so it gets regenerated alongside the .shp.
		let _ = std::fs::remove_file(shp_path.with_extension("shx"));

		let table_writer = TableWriterBuilder::new()
			.add_character_field(FieldName::try_from("name").unwrap(), 32)
			.add_numeric_field(FieldName::try_from("pop_k").unwrap(), 12, 0)
			.build_with_file_dest(&dbf_path)?;
		let shape_writer = shapefile::ShapeWriter::from_path(&shp_path)?;
		let mut writer = Writer::new(shape_writer, table_writer);

		let berlin = Polygon::with_rings(vec![PolygonRing::Outer(vec![
			Point::new(13.0, 52.3),
			Point::new(13.8, 52.3),
			Point::new(13.8, 52.7),
			Point::new(13.0, 52.7),
			Point::new(13.0, 52.3),
		])]);
		let mut berlin_record = shapefile::dbase::Record::default();
		berlin_record.insert("name".into(), FieldValue::Character(Some("Berlin".into())));
		berlin_record.insert("pop_k".into(), FieldValue::Numeric(Some(3700.0)));
		writer.write_shape_and_record(&berlin, &berlin_record)?;

		let brandenburg = Polygon::with_rings(vec![PolygonRing::Outer(vec![
			Point::new(11.5, 51.4),
			Point::new(14.8, 51.4),
			Point::new(14.8, 53.6),
			Point::new(11.5, 53.6),
			Point::new(11.5, 51.4),
		])]);
		let mut bb_record = shapefile::dbase::Record::default();
		bb_record.insert("name".into(), FieldValue::Character(Some("Brandenburg".into())));
		bb_record.insert("pop_k".into(), FieldValue::Numeric(Some(2540.0)));
		writer.write_shape_and_record(&brandenburg, &bb_record)?;

		Ok(())
	}

	/// Write a minimal shapefile + DBF whose only Character field contains a
	/// stray 0xFF byte. Returns the path to the `.shp`.
	fn write_lossy_fixture(shp_path: &Path) -> Result<()> {
		use shapefile::dbase::{FieldName, TableWriterBuilder};
		use shapefile::{Polygon, PolygonRing, Writer, record::Point};

		let dbf_path = shp_path.with_extension("dbf");
		let table_writer = TableWriterBuilder::new()
			.add_character_field(FieldName::try_from("name").unwrap(), 16)
			.build_with_file_dest(&dbf_path)?;
		let shape_writer = shapefile::ShapeWriter::from_path(shp_path)?;
		let mut writer = Writer::new(shape_writer, table_writer);

		let polygon = Polygon::with_rings(vec![PolygonRing::Outer(vec![
			Point::new(0.0, 0.0),
			Point::new(1.0, 0.0),
			Point::new(1.0, 1.0),
			Point::new(0.0, 1.0),
			Point::new(0.0, 0.0),
		])]);

		// dbase stores Character fields as fixed-width byte arrays. We can't
		// directly write non-UTF-8 strings via the high-level API, so we
		// write a placeholder, then patch the DBF after the writer flushes.
		let mut record = dbase::Record::default();
		record.insert("name".to_string(), FieldValue::Character(Some("ab__cd".to_string())));
		writer.write_shape_and_record(&polygon, &record)?;
		drop(writer);

		// Patch a non-UTF-8 byte (0xFF) into the DBF where the placeholder was.
		// DBF layout: 32-byte header + N×32 field descriptors + 1 byte terminator,
		// then records (1 byte deletion flag + field bytes per record).
		// We find the bytes "ab__cd" in the file and replace one underscore with 0xFF.
		let mut bytes = std::fs::read(&dbf_path)?;
		let needle = b"ab__cd";
		if let Some(pos) = bytes.windows(needle.len()).position(|w| w == needle) {
			bytes[pos + 2] = 0xFF;
		} else {
			panic!("placeholder not found in DBF");
		}
		std::fs::write(&dbf_path, bytes)?;
		Ok(())
	}
}
