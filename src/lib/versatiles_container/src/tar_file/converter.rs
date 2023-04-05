use crate::{TileConverterBox, TileConverterTrait, TileReaderBox};
use async_trait::async_trait;
use log::trace;
use rayon::{iter::ParallelBridge, prelude::ParallelIterator};
use std::{fs::File, path::Path, sync::Mutex};
use tar::{Builder, Header};
use versatiles_shared::{Precompression, ProgressBar, TileConverterConfig, TileFormat};

pub struct TileConverter {
	builder: Builder<File>,
	config: TileConverterConfig,
}
#[async_trait]
impl TileConverterTrait for TileConverter {
	fn new(filename: &Path, config: TileConverterConfig) -> TileConverterBox
	where
		Self: Sized,
	{
		trace!("new {:?}", filename);

		let file = File::create(filename).unwrap();
		let builder = Builder::new(file);

		Box::new(TileConverter { builder, config })
	}
	async fn convert_from(&mut self, reader: &mut TileReaderBox) {
		trace!("convert_from");

		self.config.finalize_with_parameters(reader.get_parameters());

		let tile_converter = self.config.get_tile_recompressor();

		let ext_form = match self.config.get_tile_format() {
			TileFormat::BIN => "",

			TileFormat::PNG => ".png",
			TileFormat::JPG => ".jpg",
			TileFormat::WEBP => ".webp",
			TileFormat::AVIF => ".avif",
			TileFormat::SVG => ".svg",

			TileFormat::PBF => ".pbf",
			TileFormat::GEOJSON => ".geojson",
			TileFormat::TOPOJSON => ".topojson",
			TileFormat::JSON => ".json",
		};

		let ext_comp = match self.config.get_tile_precompression() {
			Precompression::Uncompressed => "",
			Precompression::Gzip => ".gz",
			Precompression::Brotli => ".br",
		};

		let bbox_pyramide = self.config.get_bbox_pyramide();

		let meta_data = reader.get_meta().await;

		if !meta_data.is_empty() {
			let mut header = Header::new_gnu();
			header.set_size(meta_data.len() as u64);
			header.set_mode(0o644);

			self
				.builder
				.append_data(&mut header, Path::new("meta.json"), meta_data.as_slice())
				.unwrap();
		}

		let mut bar = ProgressBar::new("converting tiles", bbox_pyramide.count_tiles());
		let mutex_bar = &Mutex::new(&mut bar);
		let mutex_builder = &Mutex::new(&mut self.builder);

		for (level, bbox) in bbox_pyramide.iter_levels() {
			for row_bbox in bbox.iter_bbox_row_slices(1024) {
				let tile_vec = reader.get_bbox_tile_vec(level, &row_bbox).await;
				tile_vec.into_iter().par_bridge().for_each(|(coord, mut blob)| {
					mutex_bar.lock().unwrap().inc(1);

					blob = tile_converter.run(blob);

					let filename = format!("./{}/{}/{}{}{}", level, coord.y, coord.x, ext_form, ext_comp);
					let path = Path::new(&filename);
					let mut header = Header::new_gnu();
					header.set_size(blob.len() as u64);
					header.set_mode(0o644);

					mutex_builder
						.lock()
						.unwrap()
						.append_data(&mut header, path, blob.as_slice())
						.unwrap();
				})
			}
		}

		bar.finish();
		self.builder.finish().unwrap();
	}
}

#[cfg(test)]
mod tests {
	use super::TileConverter;
	use crate::{mbtiles::TileReader, TileConverterTrait, TileReaderTrait};
	use assert_fs::fixture::NamedTempFile;
	use std::time::Instant;
	use versatiles_shared::{Precompression, TileBBoxPyramide, TileConverterConfig, TileFormat};

	#[test]
	fn test_converter() {
		#[tokio::main]
		async fn test(tile_format: TileFormat, compression: Precompression, force_recompress: bool) {
			println!("test {:?},{:?},{:?}", tile_format, compression, force_recompress);

			let start = Instant::now();

			let mut bbox_pyramide = TileBBoxPyramide::new_full();

			// ensure test duration of < 100 ms
			match compression {
				Precompression::Uncompressed => bbox_pyramide.set_zoom_max(13),
				Precompression::Gzip => bbox_pyramide.set_zoom_max(12),
				Precompression::Brotli => bbox_pyramide.set_zoom_max(6),
			};

			let config = TileConverterConfig::new(Some(tile_format), Some(compression), bbox_pyramide, force_recompress);
			let tmp_file = NamedTempFile::new("temp.tar").unwrap();
			let mut reader = TileReader::new("../../../resources/berlin.mbtiles").await.unwrap();
			let mut convert = TileConverter::new(tmp_file.path(), config);
			convert.convert_from(&mut reader).await;
			tmp_file.close().unwrap();

			let duration = start.elapsed();
			println!("Time elapsed in expensive_function() is: {:?}", duration);
		}

		test(TileFormat::PBF, Precompression::Uncompressed, true);
		test(TileFormat::PBF, Precompression::Uncompressed, false);
		test(TileFormat::PBF, Precompression::Gzip, true);
		test(TileFormat::PBF, Precompression::Gzip, false);
		test(TileFormat::PBF, Precompression::Brotli, true);
		test(TileFormat::PBF, Precompression::Brotli, false);
	}
}
