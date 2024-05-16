use super::{parse_key, Layer};
use crate::{types::Blob, utils::BlobReader};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct VectorTile {
	pub layers: Vec<Layer>,
}

impl VectorTile {
	#[allow(dead_code)]
	pub fn from_blob(blob: Blob) -> Result<VectorTile> {
		let mut reader = BlobReader::new_le(&blob);

		let mut tile = VectorTile::default();
		while reader.data_left() {
			let (field_number, wire_type) = parse_key(reader.read_varint()?);

			println!("{field_number}, {wire_type}");

			match (field_number, wire_type) {
				(3, 2) => {
					let length = reader.read_varint()?;
					let layer_data = reader.read_blob(length)?;
					let layer = Layer::decode(&layer_data)?;
					tile.layers.push(layer);
				}
				_ => bail!("Unexpected field number or wire type".to_string()),
			}
		}
		println!("{:?}", tile);
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
		VectorTile::from_blob(blob)?;
		Ok(())
	}
}
