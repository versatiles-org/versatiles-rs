use crate::{
	opencloudtiles::{
		container::{cloudtiles, mbtiles, tar, TileConverterTrait, TileReaderBox, TileReaderTrait},
		lib::{TileBBoxPyramide, TileConverterConfig},
		server::{source, TileServer},
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
		let parts: Vec<&str> = string.split('#').collect();

		let (name, reader_source) = match parts.len() {
			1 => (guess_name(string), string.as_str()),
			2 => (parts[1], parts[0]),
			_ => panic!(),
		};

		let reader = new_reader(reader_source);
		server.add_source(
			format!("/tiles/{}/", name),
			source::TileContainer::from(reader),
		);

		fn guess_name(path: &str) -> &str {
			let filename = path.split(&['/', '\\']).last().unwrap();
			let name = filename.split('.').next().unwrap();
			name
		}
	});

	if arguments.static_folder.is_some() {
		server.add_source(
			String::from("/static/"),
			source::Folder::from(arguments.static_folder.as_ref().unwrap()),
		);
	} else if arguments.static_tar.is_some() {
		server.add_source(
			String::from("/static/"),
			source::Tar::from(arguments.static_tar.as_ref().unwrap()),
		);
	}

	let mut list: Vec<(String, String)> = server.iter_url_mapping().collect();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| println!("   {:30}  <-  {}", url.to_owned() + "*", source));

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
	let extension = filename.split('.').last().unwrap();

	let reader = match extension {
		"mbtiles" => mbtiles::TileReader::new(filename),
		"tar" => tar::TileReader::new(filename),
		"cloudtiles" => cloudtiles::TileReader::new(filename),
		_ => panic!("extension '{:?}' unknown", extension),
	};

	reader
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
		command.precompress,
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

	converter
}

fn new_server(command: &Serve) -> TileServer {
	TileServer::new(command.port)
}
