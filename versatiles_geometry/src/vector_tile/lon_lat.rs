//! Features of a vector tile in longitude and latitude, for GeoJSON export.

use anyhow::{Context, Result};
use geo::MapCoords;
use geo_types::Coord;
use versatiles_core::TileCoord;

use super::VectorTileLayer;
use crate::{ext::coord_from_mercator, geo::GeoFeature};

/// The extent MVT assumes for a layer that names none.
const DEFAULT_EXTENT: u32 = 4096;

impl VectorTileLayer {
	/// The features of this layer as they lie on the map, in degrees, for the
	/// tile at `coord`, with their ids and properties.
	///
	/// Polygon rings are classified whichever way they wind, as
	/// [`VectorTileFeature::to_feature_lenient`](super::VectorTileFeature::to_feature_lenient)
	/// does, because an export is for looking at a tile as it is.
	///
	/// # Errors
	///
	/// Errors when a feature's geometry or properties cannot be decoded.
	pub fn to_lon_lat_features(&self, coord: &TileCoord) -> Result<Vec<GeoFeature>> {
		let [x_min, y_min, x_max, y_max] = coord.to_mercator_bbox();
		let extent = f64::from(self.extent.unwrap_or(DEFAULT_EXTENT));
		let scale_x = (x_max - x_min) / extent;
		let scale_y = (y_max - y_min) / extent;
		// Tile pixels grow to the east and to the south; mercator metres to
		// the east and to the north.
		let to_lon_lat = |c: Coord<f64>| {
			coord_from_mercator(Coord {
				x: x_min + c.x * scale_x,
				y: y_max - c.y * scale_y,
			})
		};

		self
			.features
			.iter()
			.map(|feature| {
				let mut geo = feature
					.to_feature_lenient(self)
					.with_context(|| format!("decoding a feature of layer '{}'", self.name))?;
				geo.geometry = geo.geometry.map_coords(to_lon_lat);
				Ok(geo)
			})
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use geo_types::{Geometry, Point};

	use super::*;

	fn layer_with_point(x: f64, y: f64, extent: Option<u32>) -> VectorTileLayer {
		let feature = GeoFeature::new(Geometry::Point(Point::new(x, y)));
		let mut layer =
			VectorTileLayer::from_features("points".to_string(), vec![feature], extent.unwrap_or(4096), 2).unwrap();
		layer.extent = extent;
		layer
	}

	fn point(feature: &GeoFeature) -> (f64, f64) {
		match &feature.geometry {
			Geometry::Point(p) => (p.x(), p.y()),
			Geometry::MultiPoint(mp) => (mp.0[0].x(), mp.0[0].y()),
			other => panic!("not a point: {other:?}"),
		}
	}

	#[test]
	fn the_corners_of_the_world_tile_are_the_corners_of_the_map() -> Result<()> {
		let world = TileCoord::new(0, 0, 0)?;
		let (lon, lat) = point(&layer_with_point(0.0, 0.0, Some(4096)).to_lon_lat_features(&world)?[0]);
		assert!(
			(lon + 180.0).abs() < 1e-9 && (lat - 85.051_128_779_806_59).abs() < 1e-9,
			"{lon},{lat}"
		);
		let (lon, lat) = point(&layer_with_point(2048.0, 2048.0, Some(4096)).to_lon_lat_features(&world)?[0]);
		assert!(lon.abs() < 1e-9 && lat.abs() < 1e-9, "{lon},{lat}");
		Ok(())
	}

	#[test]
	fn a_point_lands_in_its_tile_and_a_missing_extent_is_4096() -> Result<()> {
		let coord = TileCoord::new(14, 8801, 5373)?;
		let (lon, lat) = point(&layer_with_point(2048.0, 2048.0, None).to_lon_lat_features(&coord)?[0]);
		assert_eq!(TileCoord::from_geo(lon, lat, 14)?, coord);
		Ok(())
	}
}
