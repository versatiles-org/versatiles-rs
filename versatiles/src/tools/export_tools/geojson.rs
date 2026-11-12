//! Vector tiles as GeoJSON, shared by `export tile` and `export area`.
//!
//! Each feature keeps its id and properties, and gains `$layer`, the layer it
//! came from, and `$tile`, the tile it was cut from. A feature that crosses
//! tile edges comes out once per tile, in pieces; `$tile` tells them apart.

use std::{collections::BTreeSet, io::Write};

use anyhow::{Context, Result};
use versatiles_container::Tile;
use versatiles_core::{GeoBBox, TileCoord};
use versatiles_geometry::geo::{GeoFeature, clip_to_bbox};

/// Decimal places of the coordinates: about 1 cm, finer than any tile at zoom 22.
const PRECISION: u8 = 7;

/// Which features to export, and how.
pub(super) struct Selection {
	/// The layers to keep. Empty keeps every layer.
	pub layers: Vec<String>,
	/// Cut every feature to this bbox.
	pub clip: Option<GeoBBox>,
}

/// A GeoJSON FeatureCollection, written feature by feature so that a large
/// area never has to fit into memory.
pub(super) struct Writer {
	out: Box<dyn Write>,
	features: u64,
	/// Every layer seen, to name in a warning when `--layer` matches none.
	layers_seen: BTreeSet<String>,
	/// Every layer kept.
	layers_kept: BTreeSet<String>,
}

impl Writer {
	pub fn new(mut out: Box<dyn Write>) -> Result<Self> {
		out.write_all(b"{\"type\":\"FeatureCollection\",\"features\":[")
			.context("writing GeoJSON")?;
		Ok(Writer {
			out,
			features: 0,
			layers_seen: BTreeSet::new(),
			layers_kept: BTreeSet::new(),
		})
	}

	/// Writes the selected features of one tile.
	pub fn write_tile(&mut self, coord: &TileCoord, tile: Tile, selection: &Selection) -> Result<()> {
		let tile_name = format!("{}/{}/{}", coord.level, coord.x, coord.y);
		let vector = tile
			.into_vector()
			.with_context(|| format!("decoding tile {tile_name}"))?;
		for layer in &vector.layers {
			self.layers_seen.insert(layer.name.clone());
			if !selection.layers.is_empty() && !selection.layers.contains(&layer.name) {
				continue;
			}
			self.layers_kept.insert(layer.name.clone());
			for mut feature in layer.to_lon_lat_features(coord)? {
				if let Some(bbox) = &selection.clip {
					let Some(geometry) = clip_to_bbox(&feature.geometry, bbox) else {
						continue;
					};
					feature.geometry = geometry;
				}
				feature.set_property("$layer".to_string(), layer.name.clone());
				feature.set_property("$tile".to_string(), tile_name.clone());
				self.write_feature(&feature)?;
			}
		}
		Ok(())
	}

	fn write_feature(&mut self, feature: &GeoFeature) -> Result<()> {
		let separator: &[u8] = if self.features == 0 { b"\n" } else { b",\n" };
		self.out.write_all(separator).context("writing GeoJSON")?;
		self
			.out
			.write_all(feature.to_json(Some(PRECISION)).stringify().as_bytes())
			.context("writing GeoJSON")?;
		self.features += 1;
		Ok(())
	}

	/// Closes the collection. Returns the number of features written.
	pub fn finish(mut self, selection: &Selection) -> Result<u64> {
		self.out.write_all(b"\n]}\n").context("writing GeoJSON")?;
		self.out.flush().context("writing GeoJSON")?;
		let missing: Vec<&String> = selection
			.layers
			.iter()
			.filter(|name| !self.layers_kept.contains(*name))
			.collect();
		if !missing.is_empty() {
			let seen: Vec<&String> = self.layers_seen.iter().collect();
			log::warn!("no tile has the layers {missing:?}; the tiles have {seen:?}");
		}
		Ok(self.features)
	}
}

#[cfg(test)]
mod tests {
	use std::{
		io,
		sync::{Arc, Mutex},
	};

	use geo_types::{Geometry, Point};
	use versatiles_core::{TileCompression, TileFormat};
	use versatiles_geometry::{
		geo::GeoValue,
		vector_tile::{VectorTile, VectorTileLayer},
	};

	use super::*;

	/// A `Write` whose bytes the test can read afterwards.
	#[derive(Clone, Default)]
	struct Shared(Arc<Mutex<Vec<u8>>>);

	impl io::Write for Shared {
		fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
			self.0.lock().unwrap().extend_from_slice(buf);
			Ok(buf.len())
		}
		fn flush(&mut self) -> io::Result<()> {
			Ok(())
		}
	}

	/// A tile with a point in the middle of layer `a` and one in layer `b`.
	fn tile() -> Tile {
		let layer = |name: &str| {
			let mut feature = GeoFeature::new(Geometry::Point(Point::new(2048.0, 2048.0)));
			feature.set_property("name".to_string(), GeoValue::from(name));
			VectorTileLayer::from_features(name.to_string(), vec![feature], 4096, 2).unwrap()
		};
		let vector = VectorTile::new(vec![layer("a"), layer("b")]);
		Tile::from_blob(
			vector.to_blob().unwrap(),
			TileCompression::Uncompressed,
			TileFormat::MVT,
		)
	}

	fn export(selection: &Selection) -> String {
		let shared = Shared::default();
		let mut writer = Writer::new(Box::new(shared.clone())).unwrap();
		writer
			.write_tile(&TileCoord::new(1, 1, 0).unwrap(), tile(), selection)
			.unwrap();
		writer.finish(selection).unwrap();
		String::from_utf8(shared.0.lock().unwrap().clone()).unwrap()
	}

	#[test]
	fn features_carry_their_layer_and_tile() {
		let json = export(&Selection {
			layers: vec![],
			clip: None,
		});
		assert!(
			json.starts_with("{\"type\":\"FeatureCollection\",\"features\":["),
			"{json}"
		);
		assert!(json.contains("\"$layer\":\"a\""), "{json}");
		assert!(json.contains("\"$layer\":\"b\""), "{json}");
		assert!(json.contains("\"$tile\":\"1/1/0\""), "{json}");
		// The middle of the north-east tile at zoom 1.
		assert!(json.contains("[90,66.5132604"), "{json}");
		versatiles_core::json::parse_json_str(&json).unwrap();
	}

	#[test]
	fn layers_and_clip_select_features() {
		let json = export(&Selection {
			layers: vec!["b".to_string()],
			clip: None,
		});
		assert!(!json.contains("\"$layer\":\"a\""), "{json}");
		assert!(json.contains("\"$layer\":\"b\""), "{json}");

		let json = export(&Selection {
			layers: vec![],
			clip: Some(GeoBBox::new(-10.0, -10.0, 10.0, 10.0).unwrap()),
		});
		assert_eq!(json, "{\"type\":\"FeatureCollection\",\"features\":[\n]}\n");
	}
}
