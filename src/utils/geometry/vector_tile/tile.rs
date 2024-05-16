use super::{parse_key, parse_varint, Layer};
use crate::types::{Blob, ByteRange};
use anyhow::{bail, Result};

#[derive(Debug, Default, PartialEq)]
pub struct VectorTile {
	pub layers: Vec<Layer>,
}

impl VectorTile {
	#[allow(dead_code)]
	pub fn from_blob(blob: Blob) -> Result<VectorTile> {
		let data = blob.as_slice();

		println!("{:?}", blob.read_range(&ByteRange::new(0, 32))?);
		let mut tile = VectorTile::default();
		let mut i = 0;
		while i < data.len() {
			let (field_number, wire_type, read_bytes) = parse_key(&data[i..])?;
			i += read_bytes;

			println!("{field_number}, {wire_type}, {read_bytes}");

			match (field_number, wire_type) {
				(3, 2) => {
					let (len, read_bytes) = parse_varint(&data[i..])?;
					i += read_bytes;
					let layer_data = &data[i..i + len as usize];
					i += len as usize;
					let layer = Layer::decode(layer_data)?;
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
