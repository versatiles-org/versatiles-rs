use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression, TileId};
use crate::{
	container::{TilesReader, TilesWriter},
	helper::{compress_gzip, progress_bar::ProgressBar, DataWriterTrait},
	types::{Blob, TileBBox},
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;

pub struct PMTilesWriter {}

#[async_trait]
impl TilesWriter for PMTilesWriter {
	async fn write_to_writer(reader: &mut dyn TilesReader, writer: &mut dyn DataWriterTrait) -> Result<()> {
		let parameters = reader.get_parameters();
		let pyramid = &parameters.bbox_pyramid;

		let mut header = HeaderV3::try_from(parameters)?;
		header.clustered = true;
		header.internal_compression = PMTilesCompression::Gzip;

		writer.append(&header.serialize()?)?;

		let mut blocks: Vec<TileBBox> = pyramid
			.iter_levels()
			.flat_map(|level_bbox| level_bbox.iter_bbox_grid(32))
			.collect();
		blocks.sort_by_cached_key(|b| b.get_tile_id().unwrap());

		let tile_count = blocks.iter().map(|block| block.count_tiles()).sum::<u64>();
		let mut progress = ProgressBar::new("converting tiles", tile_count);

		let mut addressed_tiles: u64 = 0;
		let mut offset: u64 = 0;
		let mut entries = EntriesV3::new();

		header.tile_data.offset = writer.get_position()?;

		for bbox in blocks.iter() {
			let mut tiles: Vec<(u64, Blob)> = reader
				.get_bbox_tile_stream(bbox)
				.await
				.map(|t| (t.0.get_tile_id().unwrap(), t.1))
				.collect()
				.await;

			tiles.sort_by_cached_key(|t| t.0);

			for (id, blob) in tiles {
				addressed_tiles += 1;

				entries.push(EntryV3::new(id, offset, blob.len() as u32, 1));
				offset += blob.len() as u64;
				writer.append(&blob)?;
			}

			progress.inc(bbox.count_tiles())
		}

		header.tile_data.length = offset;
		header.addressed_tiles_count = addressed_tiles;
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = entries.len() as u64;

		//setZoomCenterDefaults(&header, resolve.Entries)

		let metadata = reader.get_meta()?.unwrap_or(Blob::new_empty());
		let metadata = compress_gzip(&metadata)?;

		header.metadata = writer.append(&metadata)?;

		let directory = entries.as_directory(16384 - HeaderV3::len())?;

		header.root_dir = writer.append(&directory.root_bytes)?;

		header.leaf_dirs = writer.append(&directory.leaves_bytes)?;

		writer.write_start(&header.serialize()?)?;

		Ok(())
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use crate::{
		container::{
			mock::{MockTilesReader, MockTilesWriter},
			pmtiles::PMTilesReader,
			TilesReaderParameters,
		},
		helper::{DataReaderBlob, DataWriterBlob},
		types::{TileBBoxPyramid, TileCompression, TileFormat},
	};

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(8),
			tile_compression: TileCompression::Gzip,
			tile_format: TileFormat::PBF,
		})?;

		let mut data_writer = DataWriterBlob::new()?;
		PMTilesWriter::write_to_writer(&mut mock_reader, &mut data_writer).await?;

		let data_reader = DataReaderBlob::from(data_writer);
		let mut reader = PMTilesReader::open_reader(Box::new(data_reader)).await?;
		MockTilesWriter::write(&mut reader).await?;

		Ok(())
		// test
	}
}
