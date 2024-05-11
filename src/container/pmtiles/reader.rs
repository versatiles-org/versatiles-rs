use super::types::{Directory, EntriesV3, HeaderV3, TileId};
use crate::{
	container::{TilesReaderBox, TilesReaderParameters, TilesReaderTrait},
	helper::{DataReaderBox, DataReaderFile},
	types::{Blob, ByteRange, TileBBoxPyramid, TileCompression, TileCoord3},
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use std::{fmt::Debug, path::Path};

#[derive(Debug)]
pub struct PMTilesReader {
	data_reader: DataReaderBox,
	header: HeaderV3,
	meta: Blob,
	directory: Directory,
	parameters: TilesReaderParameters,
}

impl PMTilesReader {
	// Create a new TilesReader from a given filename
	pub async fn open_path(path: &Path) -> Result<TilesReaderBox> {
		Self::open_reader(DataReaderFile::from_path(path)?).await
	}

	pub async fn open_reader(mut data_reader: DataReaderBox) -> Result<TilesReaderBox>
	where
		Self: Sized,
	{
		let header = HeaderV3::deserialize(
			&data_reader
				.read_range(&ByteRange::new(0, HeaderV3::len() as u64))
				.await?,
		)?;

		if !header.clustered {
			bail!("source archive must be clustered for extracts");
		}

		let meta: Blob = data_reader.read_range(&header.metadata).await?;

		let directory: Directory = Directory {
			root_bytes: data_reader.read_range(&header.root_dir).await?,
			leaves_bytes: data_reader.read_range(&header.leaf_dirs).await?,
		};

		let mut bbox_pyramid = TileBBoxPyramid::new_full(header.max_zoom);
		bbox_pyramid.set_zoom_min(header.min_zoom);

		let parameters = TilesReaderParameters::new(
			header.tile_type.as_value()?,
			header.tile_compression.as_value()?,
			bbox_pyramid,
		);

		Ok(Box::new(PMTilesReader {
			data_reader,
			directory,
			header,
			meta,
			parameters,
		}))
	}
}

#[async_trait]
impl TilesReaderTrait for PMTilesReader {
	fn get_container_name(&self) -> &str {
		"pmtiles"
	}
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}
	fn override_compression(&mut self, tile_compression: TileCompression) {
		self.parameters.tile_compression = tile_compression;
	}
	fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(self.meta.clone()))
	}
	fn get_name(&self) -> &str {
		self.data_reader.get_name()
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		log::trace!("get_tile_data_original {:?}", coord);

		let tile_id: u64 = coord.get_tile_id();
		let mut dir_blob = self.directory.root_bytes.clone();

		for _depth in 0..3 {
			let entries = EntriesV3::deserialize(&dir_blob)?;
			let entry = entries.find_tile(tile_id);

			let entry = if let Some(entry) = entry {
				entry
			} else {
				return Ok(None);
			};

			if entry.range.length > 0 {
				if entry.run_length > 0 {
					return Ok(Some(
						self
							.data_reader
							.read_range(&entry.range.shift(self.header.tile_data.offset))
							.await?,
					));
				} else {
					dir_blob = self
						.data_reader
						.read_range(&entry.range.shift(self.header.leaf_dirs.offset))
						.await?;
				}
			} else {
				return Ok(None);
			}
		}

		bail!("not found")
	}
}
