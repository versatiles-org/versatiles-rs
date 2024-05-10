use super::types::{Directory, EntryV3, HeaderV3, PMTilesCompression, TileId};
use crate::{
	container::{
		DataWriterFile, DataWriterTrait, TilesReaderBox, TilesWriterBox, TilesWriterParameters, TilesWriterTrait,
	},
	helper::{compress_gzip, ProgressBar},
	types::{Blob, TileBBox},
};
use anyhow::Result;
use axum::async_trait;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};

pub struct PMTilesWriter {
	parameters: TilesWriterParameters,
	path: PathBuf,
}

impl PMTilesWriter {
	pub fn open_file(path: &Path, parameters: TilesWriterParameters) -> Result<TilesWriterBox> {
		Ok(Box::new(Self {
			parameters,
			path: path.to_owned(),
		}))
	}
}

#[async_trait]
impl TilesWriterTrait for PMTilesWriter {
	fn get_parameters(&self) -> &TilesWriterParameters {
		&self.parameters
	}

	async fn write_tiles(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		let parameters = reader.get_parameters();
		let pyramid = &parameters.bbox_pyramid;

		let mut header = HeaderV3::try_from(parameters)?;
		header.clustered = true;
		header.internal_compression = PMTilesCompression::Gzip;

		let mut file = DataWriterFile::new(&self.path)?;
		file.append(&header.serialize())?;

		let mut blocks: Vec<TileBBox> = pyramid
			.iter_levels()
			.flat_map(|level_bbox| level_bbox.iter_bbox_grid(32))
			.collect();
		blocks.sort_by_cached_key(|b| b.get_tile_id());

		let tile_count = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut progress = ProgressBar::new("converting tiles", tile_count);

		let mut addressed_tiles: u64 = 0;
		let mut offset: u64 = 0;
		let mut entries: Vec<EntryV3> = Vec::new();

		header.tile_data_offset = file.get_position()?;

		for bbox in blocks {
			let mut tiles: Vec<(u64, Blob)> = reader
				.get_bbox_tile_stream(bbox.clone())
				.await
				.map(|t| (t.0.get_tile_id(), t.1))
				.collect()
				.await;

			tiles.sort_by_cached_key(|t| t.0);

			for (id, blob) in tiles {
				addressed_tiles += 1;

				entries.push(EntryV3::new(id, offset, blob.len() as u32, 1));
				offset += blob.len() as u64;
				file.append(&blob)?;
			}

			progress.inc(bbox.count_tiles())
		}

		header.tile_data_length = offset;
		header.addressed_tiles_count = addressed_tiles;
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = entries.len() as u64;

		//setZoomCenterDefaults(&header, resolve.Entries)

		let mut metadata = reader.get_meta().await?.unwrap_or(Blob::new_empty());
		metadata = compress_gzip(metadata)?;

		header.metadata_offset = file.get_position()?;
		file.append(&metadata)?;
		header.metadata_length = metadata.len() as u64;

		let directory = Directory::new(&entries, 16384 - HeaderV3::len())?;

		header.root_dir_offset = file.get_position()?;
		file.append(&directory.root_bytes)?;
		header.root_dir_length = directory.root_bytes.len() as u64;

		header.leaf_dirs_offset = file.get_position()?;
		file.append(&directory.leaves_bytes)?;
		header.leaf_dirs_length = directory.leaves_bytes.len() as u64;

		file.write_start(&header.serialize())?;

		Ok(())
	}
}
