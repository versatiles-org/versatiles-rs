use crate::{
	opencloudtiles::{
		containers::{cloudtiles, mbtiles, tar, TileConverterTrait, TileReaderBox, TileReaderTrait},
		servers::{self, ServerSourceTileReader, TileServer},
		types::{TileBBoxPyramide, TileConverterConfig},
	},
	Compare, Convert, Probe, Serve,
};
use std::{boxed::Box, path::PathBuf};

pub fn convert(arguments: &Convert) {
	println!(
		"convert from {:?} to {:?}",
		arguments.input_file, arguments.output_file
	);

	let mut reader = new_reader(&arguments.input_file);
	let mut converter = new_converter(&arguments.output_file, arguments);
	converter.convert_from(&mut reader);
}

pub fn serve(arguments: &Serve) {
	let mut server: TileServer = new_server(arguments);

	println!("serve to http://localhost:{}/", arguments.port);

	arguments.sources.iter().for_each(|string| {
		let parts: Vec<&str> = string.split("#").collect();

		let (name, reader_source) = match parts.len() {
			1 => (guess_name(string), string.as_str()),
			2 => (parts[1], parts[0]),
			_ => panic!(),
		};

		let reader = new_reader(reader_source);
		server.add_source(
			format!("/tiles/{}/", name),
			ServerSourceTileReader::from_reader(reader),
		);

		fn guess_name(path: &str) -> &str {
			let filename = path.split(&['/', '\\']).last().unwrap();
			let name = filename.split('.').next().unwrap();
			return name;
		}
	});

	server
		.iter_url_mapping()
		.for_each(|(url, source)| println!("   - {}: {}", url, source));

	server.start();
}

pub fn probe(arguments: &Probe) {
	println!("probe {:?}", arguments.file);

	let reader = new_reader(&arguments.file);
	println!("{:#?}", reader);
}

pub fn compare(arguments: &Compare) {
	println!("compare {:?} with {:?}", arguments.file1, arguments.file2);

	let _reader1 = new_reader(&arguments.file1);
	let _reader2 = new_reader(&arguments.file2);
	todo!()
}

fn new_reader(filename: &str) -> TileReaderBox {
	let extension = filename.split(".").last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar::TileReader::new(filename),
		"cloudtiles" => cloudtiles::TileReader::new(filename),
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

	let config = TileConverterConfig::new(
		command.tile_format.clone(),
		command.precompress.clone(),
		bbox_pyramide,
		command.force_recompression,
	);

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

fn new_server(command: &Serve) -> servers::TileServer {
	servers::TileServer::new(command.port)
}
