#![allow(dead_code)]
//! Vector Tile **Tile** container.
//!
//! This module defines [`VectorTile`], a container of vector‑tile layers following the
//! Mapbox Vector Tile (MVT) protobuf schema. It provides helpers to parse a tile
//! from a binary `Blob`, serialize back to protobuf, and access layers by name.
//!
//! MVT top‑level encoding uses repeated field 3 for embedded `layer` messages.

use super::layer::VectorTileLayer;
use anyhow::{Result, bail};
use versatiles_core::{
	Blob,
	io::{ValueReader, ValueReaderSlice, ValueWriter, ValueWriterBlob},
};
use versatiles_derive::context;

/// A complete vector tile consisting of one or more layers.
///
/// Layers are stored as [`VectorTileLayer`] values and encoded/decoded using the MVT wire format.
/// This type offers ergonomic construction, (de)serialization, and lookup utilities.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorTile {
	/// The tile's layers in storage order (each one an embedded MVT `layer` message).
	pub layers: Vec<VectorTileLayer>,
}

impl VectorTile {
	/// Creates a new `VectorTile` from a vector of layers.
	#[must_use]
	pub fn new(layers: Vec<VectorTileLayer>) -> VectorTile {
		VectorTile { layers }
	}

	/// Parses a `VectorTile` from a protobuf `Blob`.
	///
	/// Iterates over the stream, reading repeated field `3` (wire‑type 2: length‑delimited)
	/// as embedded layers and delegating to [`VectorTileLayer::read`]. Returns an error
	/// for unexpected field/wire combinations or malformed input.
	#[context("parsing VectorTile from Blob ({} bytes)", blob.len())]
	pub fn from_blob(blob: &Blob) -> Result<VectorTile> {
		let mut reader = ValueReaderSlice::new_le(blob.as_slice());

		let mut tile = VectorTile::default();
		while reader.has_remaining() {
			match reader.read_pbf_key().context("Failed to read PBF key")? {
				(3, 2) => {
					tile.layers.push(
						VectorTileLayer::read(
							reader
								.get_pbf_sub_reader()
								.context("Failed to get PBF sub-reader")?
								.as_mut(),
						)
						.context("Failed to read VectorTileLayer")?,
					);
				}
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(tile)
	}

	/// Serializes this tile and all of its layers to a protobuf `Blob` (MVT wire format).
	#[context("serializing VectorTile to Blob")]
	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		for layer in &self.layers {
			writer.write_pbf_key(3, 2).context("Failed to write PBF key")?;
			writer
				.write_pbf_blob(&layer.to_blob().context("Failed to convert VectorTileLayer to blob")?)
				.context("Failed to write PBF blob")?;
		}

		Ok(writer.into_blob())
	}

	/// Returns a reference to the first layer with the given `name`, if present.
	#[must_use]
	pub fn find_layer(&self, name: &str) -> Option<&VectorTileLayer> {
		self.layers.iter().find(|layer| layer.name == name)
	}

	/// Returns a mutable reference to the first layer with the given `name`, if present.
	pub fn find_layer_mut(&mut self, name: &str) -> Option<&mut VectorTileLayer> {
		self.layers.iter_mut().find(|layer| layer.name == name)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Context;
	use std::env::current_dir;
	use versatiles_core::io::{DataReaderFile, DataReaderTrait};

	async fn get_pbf() -> Result<Blob> {
		DataReaderFile::open(&current_dir().unwrap().join("../testdata/shortbread-tile.pbf"))
			.context("Failed to open PBF file")?
			.read_all()
			.await
			.context("Failed to read all data from PBF file")
	}

	async fn get_tile() -> Result<VectorTile> {
		VectorTile::from_blob(&get_pbf().await?).context("Failed to convert blob to VectorTile")
	}

	#[tokio::test]
	async fn from_to_blob() -> Result<()> {
		let tile1 = get_tile().await.context("Failed to get initial VectorTile")?;
		let blob2 = tile1.to_blob().context("Failed to convert VectorTile to blob")?;
		let tile2 = VectorTile::from_blob(&blob2).context("Failed to convert blob back to VectorTile")?;
		assert_eq!(tile1, tile2);
		Ok(())
	}
}
