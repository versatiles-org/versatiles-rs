use super::server::{TileServer, Url};
use anyhow::Result;
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::time::{Duration, sleep};
use versatiles::{Config, TileSourceConfig, get_registry};
use versatiles_container::{ProcessingConfig, UrlPath};

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

	/// Path to a configuration file (TOML format) to configure the server, CORS, static and tile sources.
	/// Command line arguments will override configuration file settings.
	#[arg(short = 'c', long, value_name = "FILE", display_order = 0)]
	pub config: Option<PathBuf>,

	/// Serve via socket ip. Default: 0.0.0.0
	#[arg(short = 'i', long, display_order = 0)]
	pub ip: Option<String>,

	/// Serve via port. Default: 8080
	#[arg(short, long, display_order = 0)]
	pub port: Option<u16>,

	/// Serve static content at "http:/.../" from a local folder or a tar file.
	/// Tar files can be compressed (.tar / .tar.gz / .tar.br).
	/// If multiple static sources are defined, the first hit will be served.
	/// You can also add an optional url prefix like "[/assets/styles]styles.tar".
	#[arg(short = 's', long = "static", verbatim_doc_comment, display_order = 1)]
	pub static_content: Vec<String>,

	/// Shutdown server automatically after x milliseconds.
	#[arg(long, display_order = 4)]
	pub auto_shutdown: Option<u64>,

	/// use minimal recompression to reduce server response time
	#[arg(long, display_order = 2)]
	pub minimal_recompression: Option<bool>,

	/// disable API
	#[arg(long, display_order = 4)]
	pub disable_api: Option<bool>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand) -> Result<()> {
	let config = if let Some(config_path) = &arguments.config {
		Config::from_path(config_path)?
	} else {
		Config::default()
	};

	let mut server_config = config.server.unwrap_or_default();
	server_config.override_optional_ip(&arguments.ip);
	server_config.override_optional_port(&arguments.port);
	server_config.override_optional_minimal_recompression(&arguments.minimal_recompression);
	server_config.override_optional_disable_api(&arguments.disable_api);

	let registry = get_registry(ProcessingConfig::default());
	let mut server: TileServer = TileServer::from_config(server_config, registry);

	let tile_patterns: Vec<Regex> = [
		r"^\[(?P<name>[^\]]+?)\](?P<url>.*)$",
		r"^(?P<url>.*)\[(?P<name>[^\]]+?)\]$",
		r"^(?P<url>.*)#(?P<name>[^\]]+?)$",
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
		let capture = tile_patterns
			.iter()
			.find(|p| p.is_match(argument))
			.ok_or_else(|| anyhow::anyhow!("Failed to parse tile source argument: {}", argument))?
			.captures(argument)
			.ok_or_else(|| anyhow::anyhow!("Failed to parse tile source argument: {}", argument))?;

		let url = UrlPath::from(capture.name("url").unwrap().as_str());
		let name: String = match capture.name("name") {
			None => url.name()?,
			Some(m) => m.as_str().to_string(),
		};

		let tile_config = TileSourceConfig {
			name: Some(name.to_string()),
			path: UrlPath::from(url),
			flip_y: None,
			swap_xy: None,
			override_compression: None,
		};
		server.add_tile_source_config(tile_config).await?;
	}

	for tile_config in config.tile_sources {
		server.add_tile_source_config(tile_config).await?;
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
