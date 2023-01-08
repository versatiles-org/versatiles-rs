use crate::{
	opencloudtiles::{
		server::{source, TileServer}, tools::get_reader,
	}, Serve,
};

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

		let reader = get_reader(reader_source);
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

fn new_server(command: &Serve) -> TileServer {
	TileServer::new(command.port)
}
