use crate::{
	containers::{TilesReaderBox, TilesWriterBox, TilesWriterParameters, TilesWriterTrait},
	shared::{compress, compression_to_extension, format_to_extension, ProgressBar},
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use log::trace;
use std::{
	fs::File,
	path::{Path, PathBuf},
};
use tar::{Builder, Header};
use tokio::sync::Mutex;

pub struct TarTilesWriter {
	builder: Builder<File>,
	parameters: TilesWriterParameters,
}

impl TarTilesWriter {
	pub fn open_file(path: &Path, parameters: TilesWriterParameters) -> Result<TilesWriterBox>
	where
		Self: Sized,
	{
		trace!("new {:?}", path);

		ensure!(path.is_absolute(), "path {path:?} must be absolute");

		let file = File::create(path)?;
		let builder = Builder::new(file);

		Ok(Box::new(TarTilesWriter { builder, parameters }))
	}
}

#[async_trait]
impl TilesWriterTrait for TarTilesWriter {
	fn get_parameters(&self) -> &TilesWriterParameters {
		&self.parameters
	}
	async fn write_tiles(&mut self, reader: &mut TilesReaderBox) -> Result<()> {
		trace!("convert_from");
		let tile_format = &self.parameters.tile_format;
		let tile_compression = &self.parameters.tile_compression;
		let bbox_pyramid = reader.get_parameters().bbox_pyramid.clone();

		let extension_format = format_to_extension(tile_format);
		let extension_compression = compression_to_extension(tile_compression);

		let meta_data_option = reader.get_meta().await?;

		if let Some(meta_data) = meta_data_option {
			let meta_data = compress(meta_data, tile_compression)?;
			let filename = format!("tiles.json{}", extension_compression);

			let mut header = Header::new_gnu();
			header.set_size(meta_data.len() as u64);
			header.set_mode(0o644);

			self
				.builder
				.append_data(&mut header, Path::new(&filename), meta_data.as_slice())?;
		}

		let mut bar = ProgressBar::new("converting tiles", bbox_pyramid.count_tiles());
		let mutex_bar = &Mutex::new(&mut bar);
		let mutex_builder = &Mutex::new(&mut self.builder);

		for bbox in bbox_pyramid.iter_levels() {
			let mut stream = reader.get_bbox_tile_stream(*bbox).await;

			while let Some(entry) = stream.next().await {
				let (coord, blob) = entry;
				mutex_bar.lock().await.inc(1);

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

		bar.finish();
		self.builder.finish()?;

		Ok(())
	}
}
