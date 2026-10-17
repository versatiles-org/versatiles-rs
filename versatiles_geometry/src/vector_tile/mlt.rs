//! MapLibre Tile (MLT) decoding and encoding for [`VectorTile`].
//!
//! Both directions go through MVT bytes: `mlt-core` converts between its own
//! row-oriented layers and MVT, and [`VectorTile`] already reads and writes
//! MVT. That keeps this crate's model untouched by a format whose library is
//! still at 0.x, at the price of one extra protobuf pass per tile.
//!
//! What the round trip changes: `mlt-core` drops a layer without features when
//! encoding, and normalises geometry the MVT spec leaves open (consecutive
//! duplicate vertices, the closing vertex of a ring). Property values keep
//! their type where MVT has one; the integer width is inferred per column.

use anyhow::{Context, Result};
use mlt_core::{Decoder, MltError, Parser, encoder::EncoderConfig, mvt};
use versatiles_core::Blob;
use versatiles_derive::context;

use super::{VectorTile, VectorTileLayer, feature::VectorTileFeature};

impl VectorTile {
	/// Decodes an MLT tile.
	///
	/// A layer whose tag this build of `mlt-core` does not know is skipped,
	/// as the MLT spec asks of readers.
	///
	/// # Errors
	///
	/// Errors when the bytes are not an MLT tile, when a layer exceeds the
	/// decoder's 64 MiB memory budget, or when the converted MVT cannot be
	/// read.
	#[context("decoding MLT tile ({} bytes)", blob.len())]
	pub fn from_mlt_blob(blob: &Blob) -> Result<VectorTile> {
		let mut parser = Parser::default();
		let layers = parser.parse_layers(blob.as_slice()).context("parsing MLT layers")?;

		let mut decoder = Decoder::default();
		let mut tiles = Vec::with_capacity(layers.len());
		for layer in layers {
			if let Some(tile) = layer.into_tile(&mut decoder).context("decoding MLT layer")? {
				tiles.push(tile);
			}
		}

		let mvt = mvt::tile_layers_to_mvt(&tiles).context("converting MLT layers to MVT")?;
		VectorTile::from_blob(&Blob::from(mvt))
	}

	/// Encodes this tile as MLT, with `mlt-core` choosing the encodings that
	/// make it smallest.
	///
	/// The MVT reader on the way is a strict one. A geometry it refuses — a
	/// `ClosePath` with a count of 0, which this crate wrote before 5.4 and
	/// which readers in the wild accept — is decoded here and written again
	/// to the letter of the spec, for the whole tile, before a second try.
	///
	/// # Errors
	///
	/// Errors when a layer has no name, which MLT requires, when a geometry
	/// cannot be decoded at all, or when the MVT serialisation of this tile
	/// fails.
	#[context("encoding VectorTile as MLT")]
	pub fn to_mlt_blob(&self) -> Result<Blob> {
		let mvt = self.to_blob()?;
		let layers = match mvt::mvt_to_tile_layers(mvt.as_slice()) {
			Ok(layers) => layers,
			Err(MltError::Mvt(reason)) => {
				log::debug!("MLT's MVT reader refused a geometry ({reason}); rewriting the tile's geometries");
				let rewritten = self.with_rewritten_geometries()?.to_blob()?;
				mvt::mvt_to_tile_layers(rewritten.as_slice()).context("converting rewritten MVT to MLT layers")?
			}
			Err(e) => return Err(e).context("converting MVT to MLT layers"),
		};

		let mut bytes = Vec::new();
		for layer in &layers {
			bytes.extend(
				layer
					.encode(EncoderConfig::default())
					.with_context(|| format!("encoding MLT layer '{}'", layer.name()))?,
			);
		}
		Ok(Blob::from(bytes))
	}

	/// The same tile with every geometry decoded and encoded again by this
	/// crate's writer, which follows the MVT spec to the letter.
	fn with_rewritten_geometries(&self) -> Result<VectorTile> {
		let mut layers = Vec::with_capacity(self.layers.len());
		for layer in &self.layers {
			let mut features = Vec::with_capacity(layer.features.len());
			for feature in &layer.features {
				let geometry = feature
					.to_geometry()
					.with_context(|| format!("decoding a geometry in layer '{}'", layer.name))?;
				features.push(VectorTileFeature::from_geometry(
					feature.id,
					feature.tag_ids.clone(),
					geometry,
				)?);
			}
			layers.push(VectorTileLayer {
				features,
				..layer.clone()
			});
		}
		Ok(VectorTile { layers })
	}
}

#[cfg(test)]
mod tests {
	use std::env::current_dir;

	use versatiles_core::io::{DataReaderFile, DataReaderTrait};

	use super::*;
	use crate::vector_tile::VectorTileLayer;

	async fn shortbread_tile() -> Result<VectorTile> {
		let blob = DataReaderFile::open(&current_dir()?.join("../testdata/shortbread-tile.pbf"))?
			.read_all()
			.await?;
		VectorTile::from_blob(&blob)
	}

	/// What survives a round trip unchanged: the layers, their extents, how
	/// many features each holds, and every feature's id, geometry type and
	/// property count.
	fn assert_same_shape(a: &VectorTile, b: &VectorTile) {
		assert_eq!(a.layers.len(), b.layers.len(), "layer count");
		for (la, lb) in a.layers.iter().zip(&b.layers) {
			assert_eq!(la.name, lb.name);
			assert_eq!(la.extent, lb.extent, "extent of '{}'", la.name);
			assert_eq!(la.features.len(), lb.features.len(), "feature count of '{}'", la.name);
			for (fa, fb) in la.features.iter().zip(&lb.features) {
				assert_eq!(fa.id, fb.id, "feature id in '{}'", la.name);
				assert_eq!(fa.geom_type, fb.geom_type, "geometry type in '{}'", la.name);
				assert_eq!(fa.tag_ids.len(), fb.tag_ids.len(), "property count in '{}'", la.name);
			}
		}
	}

	#[tokio::test]
	async fn shortbread_tile_survives_the_round_trip() -> Result<()> {
		let tile = shortbread_tile().await?;
		let mlt = tile.to_mlt_blob()?;
		assert!(!mlt.is_empty());

		let back = VectorTile::from_mlt_blob(&mlt)?;
		assert_same_shape(&tile, &back);

		// A second trip is a fixpoint: the first one did all the normalising.
		let again = VectorTile::from_mlt_blob(&back.to_mlt_blob()?)?;
		assert_eq!(back, again);
		Ok(())
	}

	#[tokio::test]
	async fn mlt_is_smaller_than_mvt_for_a_real_tile() -> Result<()> {
		let tile = shortbread_tile().await?;
		let mvt = tile.to_blob()?;
		let mlt = tile.to_mlt_blob()?;
		assert!(
			mlt.len() < mvt.len(),
			"MLT ({} bytes) should be smaller than MVT ({} bytes)",
			mlt.len(),
			mvt.len()
		);
		Ok(())
	}

	#[test]
	fn an_empty_tile_encodes_to_nothing_and_back() -> Result<()> {
		let mlt = VectorTile::default().to_mlt_blob()?;
		assert_eq!(mlt.len(), 0);
		assert_eq!(VectorTile::from_mlt_blob(&mlt)?, VectorTile::default());
		Ok(())
	}

	#[test]
	fn a_layer_without_features_is_dropped() -> Result<()> {
		let tile = VectorTile::new(vec![VectorTileLayer::new("empty".to_string(), 4096, 2)]);
		let back = VectorTile::from_mlt_blob(&tile.to_mlt_blob()?)?;
		assert_eq!(back.layers, Vec::new());
		Ok(())
	}

	/// The stream this crate wrote before 5.4: `ClosePath` with a count of 0
	/// (the bare command 7, not 15). MLT's MVT reader refuses it, so the
	/// geometry is rewritten and the tile still encodes.
	#[test]
	fn a_close_path_without_a_count_is_rewritten() -> Result<()> {
		use versatiles_core::io::{ValueWriter, ValueWriterBlob};
		let mut writer = ValueWriterBlob::new_le();
		writer.write_varint(9)?; // MoveTo, count=1
		writer.write_svarint(10)?;
		writer.write_svarint(10)?;
		writer.write_varint((3 << 3) | 2)?; // LineTo, count=3
		for (dx, dy) in [(20, 0), (0, 20), (-20, 0)] {
			writer.write_svarint(dx)?;
			writer.write_svarint(dy)?;
		}
		writer.write_varint(7)?; // ClosePath, count=0
		let feature = VectorTileFeature {
			id: Some(1),
			tag_ids: vec![],
			geom_type: crate::vector_tile::GeomType::MultiPolygon,
			geom_data: writer.into_blob(),
		};
		let mut layer = VectorTileLayer::new("water".to_string(), 4096, 2);
		layer.features.push(feature);
		let tile = VectorTile::new(vec![layer]);

		let back = VectorTile::from_mlt_blob(&tile.to_mlt_blob()?)?;
		assert_same_shape(&tile, &back);
		assert_eq!(
			back.layers[0].features[0].to_geometry()?,
			tile.layers[0].features[0].to_geometry()?
		);
		Ok(())
	}

	#[test]
	fn garbage_is_refused() {
		let err =
			VectorTile::from_mlt_blob(&Blob::from(vec![0xff, 0xfe, 0xfd, 0x01, 0x02])).expect_err("not an MLT tile");
		assert!(format!("{err:#}").contains("MLT"), "got: {err:#}");
	}

	#[tokio::test]
	async fn geometry_types_are_kept() -> Result<()> {
		let tile = shortbread_tile().await?;
		let back = VectorTile::from_mlt_blob(&tile.to_mlt_blob()?)?;
		let kinds = |t: &VectorTile| {
			let mut v: Vec<String> = t
				.layers
				.iter()
				.flat_map(|l| l.features.iter().map(|f| format!("{:?}", f.geom_type)))
				.collect();
			v.sort();
			v.dedup();
			v
		};
		assert_eq!(kinds(&tile), kinds(&back));
		Ok(())
	}
}
