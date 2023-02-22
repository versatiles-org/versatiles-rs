use crate::{
	server::{source, TileServer},
	tools::get_reader,
};
use clap::Args;
use regex::Regex;

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// One or more tile containers you want to serve.
	/// Supported container formats are: *.versatiles, *.tar, *.mbtiles
	/// Container files have to be on the local filesystem, accept VersaTiles containers:
	///    VersaTiles containers can also be served from http://..., https://... or gs://...
	/// The name used in the url (/tiles/$name/) will be generated automatically from the file name:
	///    e.g. ".../ukraine.versatiles" will be served at url "/tiles/ukraine/..."
	/// You can also configure a different name for each file using:
	///    "[name]file", "file[name]" or "file#name"
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	pub sources: Vec<String>,

	/// serve via socket ip
	#[arg(short = 'i', long, default_value = "127.0.0.1")]
	pub ip: String,

	/// serve via port
	#[arg(short, long, default_value = "8080")]
	pub port: u16,

	/// serve static content at "/..." from a local folder
	#[arg(short = 's', long, conflicts_with = "static_tar", value_name = "folder")]
	pub static_folder: Option<String>,

	/// serve static content at "/..." from a local tar file
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
		let pattern = patterns.iter().find(|p| p.is_match(arg)).unwrap();
		let c = pattern.captures(arg).unwrap();

		let url: &str = c.name("url").unwrap().as_str();
		let name: &str = match c.name("name") {
			None => {
				let filename = url.split(&['/', '\\']).last().unwrap();
				filename.split('.').next().unwrap()
			}
			Some(m) => m.as_str(),
		};

		let reader = get_reader(url);
		server.add_source(format!("/tiles/{name}/"), source::TileContainer::from(reader));
	});

	if arguments.static_folder.is_some() {
		server.set_static(source::Folder::from(arguments.static_folder.as_ref().unwrap()));
	} else if arguments.static_tar.is_some() {
		server.set_static(source::TarFile::from(arguments.static_tar.as_ref().unwrap()));
	}

	let mut list: Vec<(String, String)> = server.iter_url_mapping().collect();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| println!("   {:30}  <-  {}", url.to_owned() + "*", source));

	server.start();
}

fn new_server(command: &Subcommand) -> TileServer {
	TileServer::new(&command.ip, command.port)
}
