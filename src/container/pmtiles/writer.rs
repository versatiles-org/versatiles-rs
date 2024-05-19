use std::sync::Arc;

use super::types::{EntriesV3, EntryV3, HeaderV3, PMTilesCompression, TileId};
use crate::{
	container::{TilesReader, TilesWriter},
	types::{progress::get_progress_bar, Blob, ByteRange, DataWriterTrait, TileBBox, TileCompression},
	utils::compress,
};
use anyhow::Result;
use async_trait::async_trait;
use futures_util::{lock::Mutex, StreamExt};

pub struct PMTilesWriter {}

#[async_trait]
impl TilesWriter for PMTilesWriter {
	async fn write_to_writer(reader: &mut dyn TilesReader, writer: &mut dyn DataWriterTrait) -> Result<()> {
		const INTERNAL_COMPRESSION: TileCompression = TileCompression::Gzip;

		let parameters = reader.get_parameters().clone();
		let pyramid = &parameters.bbox_pyramid;

		let mut blocks: Vec<TileBBox> = pyramid
			.iter_levels()
			.flat_map(|level_bbox| level_bbox.iter_bbox_grid(256))
			.collect();
		blocks.sort_by_cached_key(|b| b.get_tile_id().unwrap());

		let mut progress = get_progress_bar(
			"converting tiles",
			blocks.iter().map(|block| block.count_tiles()).sum::<u64>(),
		);
		let mut tile_count = 0;

		let entries = EntriesV3::new();

		writer.set_position(16384)?;

		let mut header = HeaderV3::from(&parameters);

		let mut metadata = reader.get_meta()?.unwrap_or(Blob::new_empty());
		metadata = compress(metadata, &INTERNAL_COMPRESSION)?;
		header.metadata = writer.append(&metadata)?;

		let tile_data_start = writer.get_position()?;

		let mutex_writer = Arc::new(Mutex::new(writer));
		let mutex_entries = Arc::new(Mutex::new(entries));

		for bbox in blocks.iter() {
			reader
				.get_bbox_tile_stream(bbox)
				.await
				.for_each(|(coord, blob)| {
					progress.inc(1);
					let mutex_writer = mutex_writer.clone();
					let mutex_entries = mutex_entries.clone();
					async move {
						let id = coord.get_tile_id().unwrap();
						let range = mutex_writer.lock().await.append(&blob).unwrap();
						mutex_entries
							.lock()
							.await
							.push(EntryV3::new(id, range.get_shifted_backward(tile_data_start), 1));
					}
				})
				.await;

			tile_count += bbox.count_tiles();
			progress.set_position(tile_count);
		}
		progress.finish();

		let mut writer = mutex_writer.lock().await;
		let mut entries = mutex_entries.lock().await;

		let tile_data_end = writer.get_position()?;

		header.tile_data = ByteRange::new(tile_data_start, tile_data_end - tile_data_start);

		writer.set_position(HeaderV3::len())?;
		let directory = entries.as_directory(16384 - HeaderV3::len(), &INTERNAL_COMPRESSION)?;
		header.root_dir = writer.append(&directory.root_bytes)?;

		writer.set_position(tile_data_end)?;
		header.leaf_dirs = writer.append(&directory.leaves_bytes)?;

		header.clustered = true;
		header.internal_compression = PMTilesCompression::from_value(INTERNAL_COMPRESSION)?;
		header.addressed_tiles_count = entries.tile_count();
		header.tile_entries_count = entries.len() as u64;
		header.tile_contents_count = entries.len() as u64;

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
		types::{DataReaderBlob, DataWriterBlob, TileBBoxPyramid, TileFormat},
	};

	#[tokio::test]
	async fn read_write() -> Result<()> {
		let mut mock_reader = MockTilesReader::new_mock(TilesReaderParameters {
			bbox_pyramid: TileBBoxPyramid::new_full(4),
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
