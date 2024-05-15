use crate::{
	container::{TilesReader, TilesWriter},
	types::{compression_to_extension, format_to_extension, progress::get_progress_bar, DataWriterTrait},
	utils::compress,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::{
	fs::File,
	path::{Path, PathBuf},
};
use tar::{Builder, Header};
use tokio::sync::Mutex;

pub struct TarTilesWriter {}

#[async_trait]
impl TilesWriter for TarTilesWriter {
	async fn write_to_path(reader: &mut dyn TilesReader, path: &Path) -> Result<()> {
		let file = File::create(path)?;
		let mut builder = Builder::new(file);

		let parameters = reader.get_parameters();
		let tile_format = &parameters.tile_format;
		let tile_compression = &parameters.tile_compression;
		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		let extension_format = format_to_extension(tile_format);
		let extension_compression = compression_to_extension(tile_compression);

		let meta_data_option = reader.get_meta()?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, tile_compression)?;
			let filename = format!("tiles.json{}", extension_compression);

			let mut header = Header::new_gnu();
			header.set_size(meta_data.len() as u64);
			header.set_mode(0o644);

			builder.append_data(&mut header, Path::new(&filename), meta_data.as_slice())?;
		}

		let mut progress = get_progress_bar("converting tiles", bbox_pyramid.count_tiles());
		let mutex_builder = &Mutex::new(&mut builder);

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(bbox).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;
				progress.inc(1);

				let filename = format!(
					"./{}/{}/{}{}{}",
					coord.get_z(),
					coord.get_y(),
					coord.get_x(),
					extension_format,
					extension_compression
				);
				let path = PathBuf::from(&filename);

				// Build header
				let mut header = Header::new_gnu();
				header.set_size(blob.len() as u64);
				header.set_mode(0o644);

				// Write blob to file
				mutex_builder
					.lock()
					.await
					.append_data(&mut header, path, blob.as_slice())?;
			}
		}

		progress.finish();
		builder.finish()?;

		Ok(())
	}
	async fn write_to_writer(_reader: &mut dyn TilesReader, _writer: &mut dyn DataWriterTrait) -> Result<()> {
		bail!("not implemented")
	}
}
