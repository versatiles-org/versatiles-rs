use crate::opencloudtiles::{
	containers::abstract_container,
	types::{TileConverterConfig, TileFormat},
};
use std::{fs::File, path::Path};
use tar::{Builder, Header};

pub struct TileConverter {
	builder: Builder<File>,
	config: TileConverterConfig,
}
impl abstract_container::TileConverter for TileConverter {
	fn new(
		filename: &std::path::PathBuf,
		config: TileConverterConfig,
	) -> Box<dyn abstract_container::TileConverter>
	where
		Self: Sized,
	{
		let file = File::create(filename).unwrap();
		let builder = Builder::new(file);

		Box::new(TileConverter { builder, config })
	}
	fn convert_from(&mut self, reader: Box<dyn abstract_container::TileReader>) {
		self
			.config
			.finalize_with_parameters(reader.get_parameters());

		let converter = self.config.get_tile_converter();

		let ext = match self.config.get_tile_format() {
			TileFormat::PBF => "pbf",
			TileFormat::PBFGzip => "pbf.gz",
			TileFormat::PBFBrotli => "pbf.br",
			TileFormat::PNG => "png",
			TileFormat::JPG => "jpg",
			TileFormat::WEBP => "webp",
			_ => panic!("unknown tile_format"),
		};
		let mut header = Header::new_gnu();
		for (z, y, x) in self.config.get_bbox_pyramide().iter_tile_indexes() {
			let tile = reader.get_tile_data(z, x, y);
			if tile.is_none() {
				continue;
			}
			let filename = format!("./{}/{}/{}.{}", z, y, x, ext);
			let path = Path::new(&filename);
			self
				.builder
				.append_data(&mut header, &path, converter(&tile.unwrap()).as_slice())
				.unwrap();
		}
		self.builder.finish().unwrap();
	}
}
