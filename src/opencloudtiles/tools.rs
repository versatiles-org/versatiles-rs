use crate::{
	opencloudtiles::{
		containers::{
			abstract_container::{TileConverterTrait, TileReaderBox, TileReaderTrait},
			cloudtiles, mbtiles, tar,
		},
		tile_server::TileServer,
		types::TileBBoxPyramide,
		types::TileConverterConfig,
	},
	Convert, Serve,
};
use std::path::PathBuf;

pub struct Tools;
impl Tools {
	pub fn convert(command: &Convert) {
		let mut reader = Tools::new_reader(&command.input_file);
		let mut converter = Tools::new_converter(&command.output_file, command);
		converter.convert_from(&mut reader);
	}
	pub fn serve(command: &Serve) {
		let mut server = Tools::new_server(command);

		command.sources.iter().for_each(|string| {
			let parts: Vec<&str> = string.split("#").collect();

			match parts.len() {
				1 => {
					server.add_source(guess_name(string), Tools::new_reader(string));
				}
				2 => {
					server.add_source(parts[1], Tools::new_reader(parts[0]));
				}
				_ => panic!(),
			}

			fn guess_name(path: &str) -> &str {
				let filename = path.split(&['/', '\\']).last().unwrap();
				let name = filename.split('.').next().unwrap();
				return name;
			}
		});

		server.start();
	}
	fn new_reader(filename: &str) -> TileReaderBox {
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
	fn new_converter(filename: &str, command: &Convert) -> Box<dyn TileConverterTrait> {
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
	fn new_server(command: &Serve) -> TileServer {
		let server = TileServer::new(command.port);

		return server;
	}
}
