use crate::{
	opencloudtiles::{
		containers::{
			abstract_container::{TileConverter, TileReader},
			cloudtiles, mbtiles, tar,
		},
		types::TileConverterConfig,
	},
	Convert, Serve,
};
use std::path::PathBuf;

use super::{server::Server, types::TileBBoxPyramide};

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) {
		let reader = Tools::new_reader(&command.input_file);
		let mut converter = Tools::new_converter(&command.output_file, command);
		converter.convert_from(reader);
	}
	pub fn serve(command: &Serve) {
		let mut server = Tools::new_server(command);

		command.sources.iter().for_each(|string| {
			let pos = string.find(":").unwrap();
			let name = string.get(0..pos).unwrap();
			let filename = string.get(pos + 1..).unwrap();
			server.add_source(name, Tools::new_reader(filename));
		});

		server.start();
	}
	fn new_reader(filename: &str) -> Box<dyn TileReader> {
		let path = PathBuf::from(filename);
		let extension = path.extension().unwrap().to_str().unwrap();

		let reader = match extension {
			"mbtiles" => mbtiles::TileReader::from_file(&path),
			"tar" => tar::TileReader::from_file(&path),
			"cloudtiles" => cloudtiles::TileReader::from_file(&path),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return reader;
	}
	fn new_converter(filename: &str, command: &Convert) -> Box<dyn TileConverter> {
		let mut bbox_pyramide = TileBBoxPyramide::new_full();

		if command.min_zoom.is_some() {
			bbox_pyramide.set_zoom_min(command.min_zoom.unwrap())
		}

		if command.max_zoom.is_some() {
			bbox_pyramide.set_zoom_max(command.max_zoom.unwrap())
		}

		if command.bbox.is_some() {
			let array = command.bbox.as_ref().unwrap().as_slice();
			bbox_pyramide.limit_by_geo_bbox(array.try_into().unwrap());
		}

		let config = TileConverterConfig::new(command.tile_format.clone(), bbox_pyramide, command.recompress);

		let path = PathBuf::from(filename);
		let extension = path.extension().unwrap().to_str().unwrap();

		let converter = match extension {
			"mbtiles" => mbtiles::TileConverter::new(&path, config),
			"cloudtiles" => cloudtiles::TileConverter::new(&path, config),
			"tar" => tar::TileConverter::new(&path, config),
			_ => panic!("extension '{:?}' unknown", extension),
		};

		return converter;
	}
	fn new_server(command: &Serve) -> Server {
		let server = Server::new(command.port);

		return server;
	}
}

fn get_extension(filename: &str) -> &str {
	let pos = filename.rfind('.').unwrap();
	return filename.get(pos + 1..).unwrap();
}
