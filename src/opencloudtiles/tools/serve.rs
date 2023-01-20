use crate::opencloudtiles::{
	server::{source, TileServer},
	tools::get_reader,
};
use clap::Args;

#[derive(Args)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	verbatim_doc_comment
)]
pub struct Subcommand {
	/// one or more tile containers you want to serve
	/// supported container formats are: *.cloudtiles, *.tar, *.mbtiles
	/// the url will be generated automatically:
	///    e.g. "ukraine.cloudtiles" will be served at url "/tiles/ukraine/..."
	/// you can add a name by using a "#":
	///    e.g. "overlay.tar#iran-revolution" will serve "overlay.tar" at url "/tiles/iran-revolution/..."
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	sources: Vec<String>,

	/// serve via port
	#[arg(short, long, default_value = "8080")]
	port: u16,

	/// serve static content at "/static/..." from folder
	#[arg(
		short = 's',
		long,
		conflicts_with = "static_tar",
		value_name = "folder"
	)]
	static_folder: Option<String>,

	/// serve static content at "/static/..." from tar file
	#[arg(
		short = 't',
		long,
		conflicts_with = "static_folder",
		value_name = "file"
	)]
	static_tar: Option<String>,
}

pub fn run(arguments: &Subcommand) {
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
			format!("/tiles/{name}/"),
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
			source::TarFile::from(arguments.static_tar.as_ref().unwrap()),
		);
	}

	let mut list: Vec<(String, String)> = server.iter_url_mapping().collect();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| println!("   {:30}  <-  {}", url.to_owned() + "*", source));

	server.start();
}

fn new_server(command: &Subcommand) -> TileServer {
	TileServer::new(command.port)
}
