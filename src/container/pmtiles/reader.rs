use super::types::{Directory, EntriesV3, HeaderV3, TileId};
use crate::{
	container::{
		versatiles::DataReaderTrait, ByteRange, DataReaderFile, TilesReaderBox, TilesReaderParameters, TilesReaderTrait,
	},
	types::{Blob, TileBBoxPyramid, TileCompression, TileCoord3},
};
use anyhow::{bail, ensure, Result};
use axum::async_trait;
use std::{fmt::Debug, path::Path};

#[derive(Debug)]
pub struct PMTilesReader {
	data_reader: Box<dyn DataReaderTrait>,
	header: HeaderV3,
	meta: Blob,
	directory: Directory,
	parameters: TilesReaderParameters,
	name: String,
}

impl PMTilesReader {
	pub async fn open(path: &Path) -> Result<TilesReaderBox>
	where
		Self: Sized,
	{
		log::trace!("read {path:?}");

		ensure!(path.is_absolute(), "path {path:?} must be absolute");
		ensure!(path.exists(), "path {path:?} does not exist");
		ensure!(path.is_file(), "path {path:?} is not a file");

		let mut data_reader = DataReaderFile::new(path)?;
		let header = HeaderV3::deserialize(
			&data_reader
				.read_range(&ByteRange::new(0, HeaderV3::len() as u64))
				.await?,
		)?;

		if !header.clustered {
			bail!("source archive must be clustered for extracts");
		}

		let meta: Blob = data_reader
			.read_range(&ByteRange::new(header.metadata_offset, header.metadata_length))
			.await?;

		let directory: Directory = Directory {
			root_bytes: data_reader
				.read_range(&ByteRange::new(header.root_dir_offset, header.root_dir_length))
				.await?,
			leaves_bytes: data_reader
				.read_range(&ByteRange::new(header.leaf_dirs_offset, header.leaf_dirs_length))
				.await?,
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
			name: path.to_str().unwrap().to_string(),
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
	async fn get_meta(&self) -> Result<Option<Blob>> {
		Ok(Some(self.meta.clone()))
	}
	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Blob> {
		log::trace!("get_tile_data_original {:?}", coord);

		let tile_id: u64 = coord.get_tile_id();
		let mut dir_blob = self.directory.root_bytes.clone();

		for _depth in 0..3 {
			let entries = EntriesV3::deserialize(&dir_blob)?;
			let entry = entries.find_tile(tile_id);

			let entry = if entry.is_none() {
				bail!("not found")
			} else {
				entry.unwrap()
			};

			if entry.length > 0 {
				if entry.run_length > 0 {
					return self
						.data_reader
						.read_range(&ByteRange::new(
							self.header.tile_data_offset + entry.offset,
							entry.length as u64,
						))
						.await;
				} else {
					dir_blob = self
						.data_reader
						.read_range(&ByteRange::new(
							self.header.leaf_dirs_offset + entry.offset,
							entry.length as u64,
						))
						.await?;
				}
			} else {
				bail!("not found")
			}
		}

		bail!("not found")
	}
	fn get_name(&self) -> &str {
		&self.name
	}
}
