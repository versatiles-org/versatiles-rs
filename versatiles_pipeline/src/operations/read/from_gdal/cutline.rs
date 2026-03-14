use anyhow::{Context, Result};
use gdal::{
	spatial_ref::SpatialRef,
	vector::{Geometry, LayerAccess},
};
use std::path::Path;
use versatiles_core::GeoBBox;

use super::get_spatial_ref;

/// A cutline polygon that clips GDAL warp output.
///
/// Stores the geometry as WKT (which is `Send + Sync`) rather than as an OGR
/// `Geometry` (which contains `RefCell` and is not `Send`).
#[derive(Clone, Debug)]
pub struct Cutline {
	/// WKT in the source dataset's SRS — recreated as OGR Geometry per warp call.
	wkt_in_source_srs: String,
	/// Bounding box of the cutline in WGS84.
	bbox_wgs84: GeoBBox,
}

impl Cutline {
	/// Build a `Cutline` from a GeoJSON file.
	///
	/// All features from the first layer are unioned into a single polygon,
	/// the WGS84 bounding box is computed, and the geometry is transformed
	/// into the source dataset's SRS.
	pub fn from_geojson(path: &Path, source_srs: &SpatialRef) -> Result<Cutline> {
		let ds = gdal::Dataset::open(path).with_context(|| format!("failed to open cutline file: {path:?}"))?;
		let mut layer = ds.layer(0).context("cutline file has no layers")?;

		// Union all feature geometries into one
		let mut result: Option<Geometry> = None;
		for feature in layer.features() {
			if let Some(geom) = feature.geometry() {
				result = Some(match result {
					None => geom.clone(),
					Some(existing) => existing.union(geom).context("failed to union cutline geometries")?,
				});
			}
		}
		let geom = result.context("cutline file contains no geometries")?;

		// Compute WGS84 bounding box
		let wgs84 = get_spatial_ref(4326)?;
		let geom_wgs84 = geom.transform_to(&wgs84)?;
		let env = geom_wgs84.envelope();
		let bbox_wgs84 = GeoBBox::new_normalized(env.MinX, env.MinY, env.MaxX, env.MaxY);

		// Transform to source dataset SRS and export as WKT
		let geom_src = geom.transform_to(source_srs)?;
		let wkt_in_source_srs = geom_src.wkt().context("failed to export cutline geometry to WKT")?;

		Ok(Cutline {
			wkt_in_source_srs,
			bbox_wgs84,
		})
	}

	/// Bounding box of the cutline in WGS84.
	pub fn bbox_wgs84(&self) -> &GeoBBox {
		&self.bbox_wgs84
	}

	/// Recreate an OGR `Geometry` from the stored WKT.
	///
	/// This must be called per warp invocation because `Geometry` is not `Send`.
	pub fn create_ogr_geometry(&self) -> Result<Geometry> {
		Geometry::from_wkt(&self.wkt_in_source_srs).context("failed to recreate cutline geometry from WKT")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::NamedTempFile;

	fn write_geojson(content: &str) -> NamedTempFile {
		let f = NamedTempFile::new("cutline.geojson").unwrap();
		std::fs::write(f.path(), content).unwrap();
		f
	}

	#[test]
	fn test_from_geojson_simple_polygon() {
		let geojson = r#"{
			"type": "FeatureCollection",
			"features": [{
				"type": "Feature",
				"geometry": {
					"type": "Polygon",
					"coordinates": [[[10.0, 48.0], [15.0, 48.0], [15.0, 52.0], [10.0, 52.0], [10.0, 48.0]]]
				},
				"properties": {}
			}]
		}"#;
		let f = write_geojson(geojson);
		let srs = get_spatial_ref(4326).unwrap();
		let cutline = Cutline::from_geojson(f.path(), &srs).unwrap();

		let bbox = cutline.bbox_wgs84();
		assert!((bbox.x_min - 10.0).abs() < 0.001);
		assert!((bbox.y_min - 48.0).abs() < 0.001);
		assert!((bbox.x_max - 15.0).abs() < 0.001);
		assert!((bbox.y_max - 52.0).abs() < 0.001);
	}

	#[test]
	fn test_from_geojson_multiple_features() {
		let geojson = r#"{
			"type": "FeatureCollection",
			"features": [
				{
					"type": "Feature",
					"geometry": {
						"type": "Polygon",
						"coordinates": [[[5.0, 45.0], [10.0, 45.0], [10.0, 50.0], [5.0, 50.0], [5.0, 45.0]]]
					},
					"properties": {}
				},
				{
					"type": "Feature",
					"geometry": {
						"type": "Polygon",
						"coordinates": [[[8.0, 48.0], [15.0, 48.0], [15.0, 55.0], [8.0, 55.0], [8.0, 48.0]]]
					},
					"properties": {}
				}
			]
		}"#;
		let f = write_geojson(geojson);
		let srs = get_spatial_ref(4326).unwrap();
		let cutline = Cutline::from_geojson(f.path(), &srs).unwrap();

		let bbox = cutline.bbox_wgs84();
		// Union of the two polygons: x=[5,15], y=[45,55]
		assert!((bbox.x_min - 5.0).abs() < 0.001);
		assert!((bbox.y_min - 45.0).abs() < 0.001);
		assert!((bbox.x_max - 15.0).abs() < 0.001);
		assert!((bbox.y_max - 55.0).abs() < 0.001);
	}

	#[test]
	fn test_create_ogr_geometry() {
		let geojson = r#"{
			"type": "FeatureCollection",
			"features": [{
				"type": "Feature",
				"geometry": {
					"type": "Polygon",
					"coordinates": [[[10.0, 48.0], [15.0, 48.0], [15.0, 52.0], [10.0, 52.0], [10.0, 48.0]]]
				},
				"properties": {}
			}]
		}"#;
		let f = write_geojson(geojson);
		let srs = get_spatial_ref(4326).unwrap();
		let cutline = Cutline::from_geojson(f.path(), &srs).unwrap();

		let geom = cutline.create_ogr_geometry().unwrap();
		assert!(!geom.is_empty());
	}

	#[test]
	fn test_from_geojson_transform_to_mercator() {
		let geojson = r#"{
			"type": "FeatureCollection",
			"features": [{
				"type": "Feature",
				"geometry": {
					"type": "Polygon",
					"coordinates": [[[10.0, 48.0], [15.0, 48.0], [15.0, 52.0], [10.0, 52.0], [10.0, 48.0]]]
				},
				"properties": {}
			}]
		}"#;
		let f = write_geojson(geojson);
		let srs = get_spatial_ref(3857).unwrap();
		let cutline = Cutline::from_geojson(f.path(), &srs).unwrap();

		// bbox_wgs84 should still be in WGS84
		let bbox = cutline.bbox_wgs84();
		assert!((bbox.x_min - 10.0).abs() < 0.001);
		assert!((bbox.y_min - 48.0).abs() < 0.001);

		// But the WKT geometry should be in Mercator (large coordinate values)
		let geom = cutline.create_ogr_geometry().unwrap();
		let env = geom.envelope();
		// 10 degrees ~ 1113195 meters in Mercator
		assert!(env.MinX > 1_000_000.0);
	}

	#[test]
	fn test_empty_geojson_fails() {
		let geojson = r#"{
			"type": "FeatureCollection",
			"features": []
		}"#;
		let f = write_geojson(geojson);
		let srs = get_spatial_ref(4326).unwrap();
		let result = Cutline::from_geojson(f.path(), &srs);
		assert!(result.is_err());
	}

	#[test]
	fn test_cutline_is_send_sync() {
		fn assert_send_sync<T: Send + Sync>() {}
		assert_send_sync::<Cutline>();
	}
}
