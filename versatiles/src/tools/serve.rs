use super::server::{TileServer, Url};
use anyhow::Result;
use regex::Regex;
use std::path::Path;
use tokio::time::{Duration, sleep};
use versatiles::get_registry;
use versatiles_container::{ProcessingConfig, TilesConvertReader, TilesConverterParameters, TilesReaderTrait};
use versatiles_core::TileCompression;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// One or more tile containers you want to serve.
	/// Supported container formats are: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory
	/// Container files have to be on the local filesystem, except VersaTiles containers:
	///    VersaTiles containers can also be served from http://... or https://...
	/// The id used in the url (/tiles/$id/) will be generated automatically from the file id:
	///    e.g. ".../ukraine.versatiles" will be served at url "/tiles/ukraine/..."
	/// You can also configure a different id for each file using:
	///    "[id]file", "file[id]" or "file#id"
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	pub tile_sources: Vec<String>,

	/// Serve via socket ip.
	#[arg(short = 'i', long, default_value = "0.0.0.0", display_order = 0)]
	pub ip: String,

	/// Serve via port.
	#[arg(short, long, default_value = "8080", display_order = 0)]
	pub port: u16,

	/// Serve static content at "http:/.../" from a local folder or a tar file.
	/// Tar files can be compressed (.tar / .tar.gz / .tar.br).
	/// If multiple static sources are defined, the first hit will be served.
	/// You can also add an optional url prefix like "[/assets/styles]styles.tar".
	#[arg(short = 's', long = "static", verbatim_doc_comment, display_order = 1)]
	pub static_content: Vec<String>,

	/// Shutdown server automatically after x milliseconds.
	#[arg(long, display_order = 4)]
	pub auto_shutdown: Option<u64>,

	/// swap rows and columns, e.g. z/x/y -> z/y/x
	#[arg(long, display_order = 3)]
	pub swap_xy: bool,

	/// flip input vertically
	#[arg(long, display_order = 3)]
	pub flip_y: bool,

	/// use minimal recompression to reduce server response time
	#[arg(long, display_order = 2)]
	pub minimal_recompression: bool,

	/// disable API
	#[arg(long, display_order = 4)]
	pub disable_api: bool,

	/// override the compression of the input source, e.g. to handle gzipped tiles in a tar, that do not end in .gz
	/// (deprecated in favor of a better solution that does not yet exist)
	#[arg(long, value_enum, value_name = "COMPRESSION", display_order = 4)]
	override_input_compression: Option<TileCompression>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	let mut server: TileServer = TileServer::new(
		&arguments.ip,
		arguments.port,
		arguments.minimal_recompression,
		!arguments.disable_api,
	);

	let tile_patterns: Vec<Regex> = [
		r"^\[(?P<id>[^\]]+?)\](?P<url>.*)$",
		r"^(?P<url>.*)\[(?P<id>[^\]]+?)\]$",
		r"^(?P<url>.*)#(?P<id>[^\]]+?)$",
		r"^(?P<url>.*)$",
	]
	.iter()
	.map(|pat| Regex::new(pat).unwrap())
	.collect();

	let static_patterns: Vec<Regex> = [
		r"^\[(?P<path>[^\]]+?)\](?P<filename>.*)$",
		r"^(?P<filename>.*)\[(?P<path>[^\]]+?)\]$",
		r"^(?P<filename>.*)$",
	]
	.iter()
	.map(|pat| Regex::new(pat).unwrap())
	.collect();

	for argument in arguments.tile_sources.iter() {
		// parse url: Does it also contain a "id" or other parameters?
		let capture = tile_patterns
			.iter()
			.find(|p| p.is_match(argument))
			.unwrap()
			.captures(argument)
			.unwrap();

		let url: &str = capture.name("url").unwrap().as_str();
		let id: &str = match capture.name("id") {
			None => url.split(&['/', '\\']).next_back().unwrap().split('.').next().unwrap(),
			Some(m) => m.as_str(),
		};

		let mut reader = get_registry(ProcessingConfig::default()).get_reader(url).await?;

		if arguments.override_input_compression.is_some() {
			reader.override_compression(arguments.override_input_compression.unwrap())
		}

		if arguments.flip_y || arguments.swap_xy {
			let cp = TilesConverterParameters {
				flip_y: arguments.flip_y,
				swap_xy: arguments.swap_xy,
				..Default::default()
			};
			reader = TilesConvertReader::new_from_reader(reader, cp)?.boxed();
		}

		server.add_tile_source(id, reader)?;
	}

	for argument in arguments.static_content.iter() {
		let capture = static_patterns
			.iter()
			.find(|p| p.is_match(argument))
			.unwrap()
			.captures(argument)
			.unwrap();

		let filename: &str = capture.name("filename").unwrap().as_str();
		let url_prefix: &str = match capture.name("path") {
			None => "",
			Some(m) => m.as_str(),
		};

		server.add_static_source(Path::new(filename), Url::new(url_prefix))?;
	}

	let mut list: Vec<(String, String)> = server.get_url_mapping().await;
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| eprintln!("   {:30}  <-  {}", url.to_owned() + "*", source));

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
	use anyhow::Result;

	#[test]
	fn test_local() -> Result<()> {
		run_command(vec![
			"versatiles",
			"serve",
			"-i",
			"127.0.0.1",
			"-p",
			"65001",
			"--auto-shutdown",
			"500",
			"../testdata/berlin.mbtiles[test]",
		])?;
		Ok(())
	}

	#[test]
	fn test_remote() -> Result<()> {
		run_command(vec![
			"versatiles",
			"serve",
			"-i",
			"127.0.0.1",
			"-p",
			"65002",
			"--auto-shutdown",
			"500",
			"[test]https://download.versatiles.org/osm.versatiles",
		])?;
		Ok(())
	}
}
