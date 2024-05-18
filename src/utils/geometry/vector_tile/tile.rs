use super::{layer::VectorTileLayer, utils::BlobReaderPBF};
use crate::{types::Blob, utils::BlobReader};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct VectorTile {
	pub layers: Vec<VectorTileLayer>,
}

impl VectorTile {
	#[allow(dead_code)]
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
}

#[cfg(test)]
mod test {
	use super::VectorTile;
	use crate::{
		container::{pmtiles::PMTilesReader, TilesReader},
		types::TileCoord3,
		utils::decompress,
	};
	use anyhow::Result;
	use lazy_static::lazy_static;
	use std::{env::current_dir, path::PathBuf};

	lazy_static! {
		static ref PATH: PathBuf = current_dir().unwrap().join("./testdata/berlin.pmtiles");
	}

	#[tokio::test]
	async fn from_blob() -> Result<()> {
		let mut reader = PMTilesReader::open_path(&PATH).await?;
		let mut blob = reader.get_tile_data(&TileCoord3::new(8803, 5376, 14)?).await?.unwrap();
		blob = decompress(blob, &reader.get_parameters().tile_compression)?;
		VectorTile::from_blob(&blob)?;
		//println!("{:?}", tile);
		Ok(())
	}
}
