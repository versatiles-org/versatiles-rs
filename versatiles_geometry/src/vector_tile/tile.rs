#![allow(dead_code)]

use super::layer::VectorTileLayer;
use anyhow::{Context, Result, bail};
use versatiles_core::{Blob, io::*};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorTile {
	pub layers: Vec<VectorTileLayer>,
}

impl VectorTile {
	pub fn new(layers: Vec<VectorTileLayer>) -> VectorTile {
		VectorTile { layers }
	}

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

	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = ValueWriterBlob::new_le();

		for layer in self.layers.iter() {
			writer.write_pbf_key(3, 2).context("Failed to write PBF key")?;
			writer
				.write_pbf_blob(&layer.to_blob().context("Failed to convert VectorTileLayer to blob")?)
				.context("Failed to write PBF blob")?;
		}

		Ok(writer.into_blob())
	}

	pub fn find_layer(&self, name: &str) -> Option<&VectorTileLayer> {
		self.layers.iter().find(|layer| layer.name == name)
	}

	pub fn find_layer_mut(&mut self, name: &str) -> Option<&mut VectorTileLayer> {
		self.layers.iter_mut().find(|layer| layer.name == name)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::env::current_dir;

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
