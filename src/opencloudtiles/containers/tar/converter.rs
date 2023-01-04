use crate::opencloudtiles::{
	containers::abstract_container::{TileConverterTrait, TileReaderBox},
	helpers::ProgressBar,
	types::{TileConverterConfig, TileFormat, TilePrecompression},
};
use rayon::{iter::ParallelBridge, prelude::ParallelIterator};
use std::{fs::File, path::Path, sync::Mutex};
use tar::{Builder, Header};

pub struct TileConverter {
	builder: Builder<File>,
	config: TileConverterConfig,
}
impl TileConverterTrait for TileConverter {
	fn new(filename: &std::path::PathBuf, config: TileConverterConfig) -> Box<dyn TileConverterTrait>
	where
		Self: Sized,
	{
		let file = File::create(filename).unwrap();
		let builder = Builder::new(file);

		Box::new(TileConverter { builder, config })
	}
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		self
			.config
			.finalize_with_parameters(reader.get_parameters());

		let tile_converter = self.config.get_tile_recompressor();

		let ext_form = match self.config.get_tile_format() {
			TileFormat::PBF => ".pbf",
			TileFormat::PNG => ".png",
			TileFormat::JPG => ".jpg",
			TileFormat::WEBP => ".webp",
		};

		let ext_comp = match self.config.get_tile_precompression() {
			TilePrecompression::Uncompressed => "",
			TilePrecompression::Gzip => ".gz",
			TilePrecompression::Brotli => ".br",
		};

		let bbox_pyramide = self.config.get_bbox_pyramide();

		let meta_data = reader.get_meta();
		if meta_data.len() > 0 {
			let mut header = Header::new_gnu();
			header.set_size(meta_data.len() as u64);
			header.set_mode(0o644);

			self
				.builder
				.append_data(&mut header, &Path::new("meta.json"), meta_data)
				.unwrap();
		}

		let mut bar = ProgressBar::new("converting tiles", bbox_pyramide.count_tiles());
		let mutex_bar = &Mutex::new(&mut bar);
		let mutex_reader = &Mutex::new(reader);
		let mutex_builder = &Mutex::new(&mut self.builder);

		bbox_pyramide
			.iter_tile_indexes()
			.par_bridge()
			.for_each(|coord| {
				// println!("{:?}", coord);

				mutex_bar.lock().unwrap().inc(1);

				let tile = mutex_reader.lock().unwrap().get_tile_data(&coord);
				if tile.is_none() {
					return;
				}

				let mut tile = tile.unwrap();

				for converter in tile_converter.iter() {
					tile = converter(&tile);
				}

				let filename = format!(
					"./{}/{}/{}{}{}",
					coord.z, coord.y, coord.x, ext_form, ext_comp
				);
				let path = Path::new(&filename);
				let mut header = Header::new_gnu();
				header.set_size(tile.len() as u64);
				header.set_mode(0o644);

				mutex_builder
					.lock()
					.unwrap()
					.append_data(&mut header, &path, tile.as_slice())
					.unwrap();
			});

		bar.finish();
		self.builder.finish().unwrap();
	}
}
