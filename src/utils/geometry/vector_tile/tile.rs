#![allow(dead_code)]

use super::{
	layer::VectorTileLayer,
	utils::{BlobReaderPBF, BlobWriterPBF},
};
use crate::{
	types::Blob,
	utils::{BlobReader, BlobWriter},
};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct VectorTile {
	pub layers: Vec<VectorTileLayer>,
}

impl VectorTile {
	pub fn from_blob(blob: &Blob) -> Result<VectorTile> {
		let mut reader = BlobReader::new_le(blob);

		let mut tile = VectorTile::default();
		while reader.has_remaining() {
			match reader.read_pbf_key()? {
				(3, 2) => {
					tile
						.layers
						.push(VectorTileLayer::read(&mut reader.get_pbf_sub_reader()?)?);
				}
				(f, w) => bail!("Unexpected combination of field number ({f}) and wire type ({w})"),
			}
		}

		Ok(tile)
	}
	pub fn to_blob(&self) -> Result<Blob> {
		let mut writer = BlobWriter::new_le();

		for layer in self.layers.iter() {
			writer.write_pbf_key(3, 2)?;
			writer.write_pbf_blob(&layer.to_blob()?)?;
		}

		Ok(writer.into_blob())
	}
}

#[cfg(test)]
mod test {
	use anyhow::Context;

	use super::*;
	use crate::types::{DataReaderFile, DataReaderTrait};
	use std::env::current_dir;

	async fn get_pbf() -> Result<Blob> {
		DataReaderFile::open(&current_dir().unwrap().join("./testdata/shortbread-tile.pbf"))?
			.read_all()
			.await
	}

	#[tokio::test]
	async fn from_to_blob() -> Result<()> {
		let blob1 = get_pbf().await.context("get pbf")?;
		let tile1 = VectorTile::from_blob(&blob1).context("from blob 1")?;

		let blob2 = tile1.to_blob().context("to blob")?;
		let tile2 = VectorTile::from_blob(&blob2).context("from blob 2")?;
		assert_eq!(tile1, tile2);
		Ok(())
	}
}
