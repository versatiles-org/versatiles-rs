use crate::{
	server::{source, TileServer},
	tools::get_reader,
};
use clap::Args;
use regex::Regex;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// one or more tile containers you want to serve
	/// supported container formats are: *.versatiles, *.tar, *.mbtiles
	/// the url will be generated automatically:
	///    e.g. "ukraine.versatiles" will be served at url "/tiles/ukraine/..."
	/// you can add a name by using a "#":
	///    e.g. "overlay.tar#iran-revolution" will serve "overlay.tar" at url "/tiles/iran-revolution/..."
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	pub sources: Vec<String>,

	/// serve via port
	#[arg(short, long, default_value = "8080")]
	pub port: u16,

	/// serve static content at "/static/..." from folder
	#[arg(short = 's', long, conflicts_with = "static_tar", value_name = "folder")]
	pub static_folder: Option<String>,

	/// serve static content at "/static/..." from tar file
	#[arg(short = 't', long, conflicts_with = "static_folder", value_name = "file")]
	pub static_tar: Option<String>,
}

pub fn run(arguments: &Subcommand) {
	let mut server: TileServer = new_server(arguments);

	let patterns: Vec<Regex> = [
		r"^\[(?P<name>[a-z0-9-]+?)\](?P<url>.*)$",
		r"^(?P<url>.*)\[(?P<name>[a-z0-9-]+?)\]$",
		r"^(?P<url>.*)#(?P<name>[a-z0-9-]+?)$",
		r"^(?P<url>.*)$",
	]
	.iter()
	.map(|pat| Regex::new(pat).unwrap())
	.collect();

	arguments.sources.iter().for_each(|arg| {
		let name: &str;
		let url: &str;

		match patterns.iter().find(|p| p.is_match(arg)) {
			None => panic!(),
			Some(pattern) => {
				let c = pattern.captures(&arg).unwrap();
				url = c.name("url").unwrap().as_str();
				name = match c.name("name") {
					None => {
						let filename = url.split(&['/', '\\']).last().unwrap();
						filename.split('.').next().unwrap()
					}
					Some(m) => m.as_str(),
				}
			}
		}

		let reader = get_reader(url);
		server.add_source(format!("/tiles/{name}/"), source::TileContainer::from(reader));
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

	println!("listening on http://127.0.0.1:{}/", arguments.port);
	server.start();
}

fn new_server(command: &Subcommand) -> TileServer {
	TileServer::new(command.port)
}
