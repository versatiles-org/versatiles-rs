use crate::{container::*, helper::*};
use log::trace;
use rayon::{iter::ParallelBridge, prelude::ParallelIterator};
use std::{fs::File, path::Path, sync::Mutex};
use tar::{Builder, Header};

pub struct TileConverter {
	builder: Builder<File>,
	config: TileConverterConfig,
}
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
	fn convert_from(&mut self, reader: &mut TileReaderBox) {
		trace!("convert_from");

		self
			.config
			.finalize_with_parameters(reader.get_parameters());

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

		let meta_data = reader.get_meta();

		if meta_data.is_empty() {
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

		bbox_pyramide.iter_levels().for_each(|(level, bbox)| {
			bbox
				.iter_bbox_row_slices(1024)
				.for_each(|row_bbox: TileBBox| {
					let tile_vec = reader.get_bbox_tile_vec(level, &row_bbox);
					tile_vec
						.into_iter()
						.par_bridge()
						.for_each(|(coord, mut blob)| {
							mutex_bar.lock().unwrap().inc(1);

							blob = tile_converter.run(blob);

							let filename = format!(
								"./{}/{}/{}{}{}",
								level, coord.y, coord.x, ext_form, ext_comp
							);
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
				})
		});

		bar.finish();
		self.builder.finish().unwrap();
	}
}
