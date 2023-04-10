use crate::{
	containers::get_reader,
	server::{source, TileServer},
	shared::Result,
};
use clap::Args;
use regex::Regex;
use tokio::time::{sleep, Duration};

#[derive(Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// One or more tile containers you want to serve.
	/// Supported container formats are: *.versatiles, *.tar, *.mbtiles
	/// Container files have to be on the local filesystem, except VersaTiles containers:
	///    VersaTiles containers can also be served from http://..., https://... or gs://...
	/// The name used in the url (/tiles/$name/) will be generated automatically from the file name:
	///    e.g. ".../ukraine.versatiles" will be served at url "/tiles/ukraine/..."
	/// You can also configure a different name for each file using:
	///    "[name]file", "file[name]" or "file#name"
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	pub sources: Vec<String>,

	/// Serve via socket ip.
	#[arg(short = 'i', long, default_value = "127.0.0.1")]
	pub ip: String,

	/// Serve via port.
	#[arg(short, long, default_value = "8080")]
	pub port: u16,

	/// Serve static content at "http:/.../" from a local folder or tar.
	/// If multiple static sources are defined, the first hit will be served.
	#[arg(short = 's', long = "static", verbatim_doc_comment)]
	pub static_content: Vec<String>,

	/// Shutdown server automatically after x milliseconds.
	#[arg(long)]
	pub auto_shutdown: Option<u64>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	let mut server: TileServer = TileServer::new(&arguments.ip, arguments.port);

	let patterns: Vec<Regex> = [
		r"^\[(?P<name>[^\]]+?)\](?P<url>.*)$",
		r"^(?P<url>.*)\[(?P<name>[^\]]+?)\]$",
		r"^(?P<url>.*)#(?P<name>[^\]]+?)$",
		r"^(?P<url>.*)$",
	]
	.iter()
	.map(|pat| Regex::new(pat).unwrap())
	.collect();

	for arg in arguments.sources.iter() {
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

		let reader = get_reader(url).await?;
		let source = source::TileContainer::from(reader)?;
		server.add_tile_source(&format!("/tiles/{name}/"), source);
	}

	for filename in arguments.static_content.iter() {
		if filename.ends_with(".tar") {
			server.add_static_source(source::TarFile::from(filename));
		} else {
			server.add_static_source(source::Folder::from(filename));
		}
	}

	let mut list: Vec<(String, String)> = server.iter_url_mapping().collect();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| println!("   {:30}  <-  {}", url.to_owned() + "*", source));

	server.start().await?;

	if let Some(milliseconds) = arguments.auto_shutdown {
		sleep(Duration::from_millis(milliseconds)).await
	} else {
		loop {
			sleep(Duration::from_secs(60)).await
		}
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;

	#[test]
	fn test_local() {
		run_command(vec![
			"versatiles",
			"serve",
			"-p",
			"65001",
			"--auto-shutdown",
			"500",
			"ressources/berlin.mbtiles",
		])
		.unwrap();
	}

	#[test]
	fn test_remote() {
		run_command(vec![
			"versatiles",
			"serve",
			"-p",
			"65002",
			"--auto-shutdown",
			"500",
			"https://download.versatiles.org/planet-20230227.versatiles",
		])
		.unwrap();
	}
}
