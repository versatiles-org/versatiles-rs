use super::types::{Directory, EntryV3, HeaderV3, PMTilesCompression, TileId};
use crate::{
	container::{TilesReaderBox, TilesWriterBox, TilesWriterTrait},
	helper::{compress_gzip, progress_bar::ProgressBar, DataWriterFile, DataWriterTrait},
	types::{Blob, TileBBox},
};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use std::path::{Path, PathBuf};

pub struct PMTilesWriter {
	path: PathBuf,
}

impl PMTilesWriter {
	pub fn open_path(path: &Path) -> Result<TilesWriterBox> {
		Ok(Box::new(Self { path: path.to_owned() }))
	}
}

#[async_trait]
impl TilesWriterTrait for PMTilesWriter {
	async fn write_from_reader(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
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

		header.tile_data.offset = file.get_position()?;

		for bbox in blocks.iter() {
			let mut tiles: Vec<(u64, Blob)> = reader
				.get_bbox_tile_stream(bbox)
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

		header.tile_data.length = offset;
		header.addressed_tiles_count = addressed_tiles;
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = entries.len() as u64;

		//setZoomCenterDefaults(&header, resolve.Entries)

		let mut metadata = reader.get_meta()?.unwrap_or(Blob::new_empty());
		metadata = compress_gzip(metadata)?;

		header.metadata = file.append(&metadata)?;

		let directory = Directory::new(&entries, 16384 - HeaderV3::len())?;

		header.root_dir = file.append(&directory.root_bytes)?;

		header.leaf_dirs = file.append(&directory.leaves_bytes)?;

		file.write_start(&header.serialize())?;

		Ok(())
	}
}
