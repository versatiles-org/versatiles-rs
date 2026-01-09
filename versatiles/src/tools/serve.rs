use anyhow::{Context, Result};
use regex::Regex;
use std::{mem::swap, path::PathBuf};
use tokio::time::{Duration, sleep};
use versatiles::{
	config::{Config, StaticSourceConfig, TileSourceConfig},
	server::TileServer,
};
use versatiles_container::{DataLocation, DataSource, TilesRuntime};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// One or more tile containers to serve (path, URL, or data source expression).
	///
	/// Supported formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory.
	/// Only VersaTiles containers can be served from remote URLs (http/https).
	/// The URL path (/tiles/{id}/) is derived from the source name:
	///    e.g. "ukraine.versatiles" -> "/tiles/ukraine/..."
	/// Override the name using bracket notation:
	///    "[osm]tiles.versatiles"  or  "tiles.versatiles[osm]"
	/// Run `versatiles help source` for full syntax details.
	#[arg(verbatim_doc_comment)]
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
pub async fn run(arguments: &Subcommand, runtime: TilesRuntime) -> Result<()> {
	let mut config = if let Some(config_path) = &arguments.config {
		Config::from_path(config_path)
			.context("run `versatiles help config` to get more information about the config file format")?
	} else {
		Config::default()
	};

	config.server.override_optional_ip(&arguments.ip);
	config.server.override_optional_port(&arguments.port);
	config
		.server
		.override_optional_minimal_recompression(&arguments.minimal_recompression);
	config.server.override_optional_disable_api(&arguments.disable_api);

	for src in &arguments.tile_sources {
		let src = DataSource::parse(src)?;
		config.tile_sources.push(TileSourceConfig { name: None, src });
	}

	let static_patterns: Vec<Regex> = [
		r"^\[(?P<path>[^\]]+?)\](?P<filename>.*)$",
		r"^(?P<filename>.*)\[(?P<path>[^\]]+?)\]$",
		r"^(?P<filename>.*)$",
	]
	.iter()
	.map(|pat| Regex::new(pat).unwrap())
	.collect();

	let mut static_sources = arguments
		.static_content
		.iter()
		.map(|argument| {
			let capture = static_patterns
				.iter()
				.find(|p| p.is_match(argument))
				.unwrap()
				.captures(argument)
				.unwrap();

			let filename: &str = capture.name("filename").unwrap().as_str();
			let prefix = capture.name("path").map(|m| m.as_str().to_string());

			Ok(StaticSourceConfig {
				src: DataLocation::parse(filename)?,
				prefix,
			})
		})
		.collect::<Result<Vec<StaticSourceConfig>>>()?;
	swap(&mut config.static_sources, &mut static_sources);
	config.static_sources.extend(static_sources);

	let mut server: TileServer = TileServer::from_config(config, runtime).await?;

	let mut list = server.get_url_mapping();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
	list
		.iter()
		.for_each(|(url, source)| log::info!("add tile source: {} <- {source}", url.join_as_string("*")));

	server.start().await?;

	if let Some(milliseconds) = arguments.auto_shutdown {
		sleep(Duration::from_millis(milliseconds)).await;
	} else {
		loop {
			sleep(Duration::from_secs(60)).await;
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

	#[test]
	fn test_config() -> Result<()> {
		run_command(vec![
			"versatiles",
			"serve",
			"-c",
			"../testdata/config1.yml",
			"-p",
			"65003",
			"--auto-shutdown",
			"500",
		])?;
		Ok(())
	}
}
