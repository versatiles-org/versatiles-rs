use crate::opencloudtiles::{
	containers::abstract_container::{TileConverterTrait, TileReaderBox},
	progress::ProgressBar,
	types::{TileConverterConfig, TileFormat},
};
use rayon::iter::ParallelBridge;
use rayon::prelude::ParallelIterator;
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
		self.config.finalize_with_parameters(reader.get_parameters());

		let converter = self.config.get_tile_converter();

		let ext = match self.config.get_tile_format() {
			TileFormat::PBF => "pbf",
			TileFormat::PBFGzip => "pbf.gz",
			TileFormat::PBFBrotli => "pbf.br",
			TileFormat::PNG => "png",
			TileFormat::JPG => "jpg",
			TileFormat::WEBP => "webp",
		};

		let bbox_pyramide = self.config.get_bbox_pyramide();
		//println!("{:?}", bbox_pyramide);

		let mut bar = ProgressBar::new("counting tiles", bbox_pyramide.count_tiles());
		let mutex_bar = &Mutex::new(&mut bar);
		let mutex_reader = &Mutex::new(reader);
		let mutex_builder = &Mutex::new(&mut self.builder);

		bbox_pyramide.iter_tile_indexes().par_bridge().for_each(|coord| {
			// println!("{:?}", coord);

			mutex_bar.lock().unwrap().inc(1);

			let tile = mutex_reader.lock().unwrap().get_tile_data(&coord);
			if tile.is_none() {
				return;
			}

			let tile_data = tile.unwrap();
			let tile_compressed = converter(&tile_data);

			//println!("{}", &tile_data.len());

			let filename = format!("./{}/{}/{}.{}", coord.z, coord.y, coord.x, ext);
			let path = Path::new(&filename);
			let mut header = Header::new_gnu();
			header.set_size(tile_compressed.len() as u64);
			header.set_mode(0o644);

			mutex_builder
				.lock()
				.unwrap()
				.append_data(&mut header, &path, tile_compressed.as_slice())
				.unwrap();
		});

		bar.finish();
		self.builder.finish().unwrap();
	}
}
