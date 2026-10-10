use std::{mem::swap, path::PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use tokio::{
	sync::mpsc::{UnboundedReceiver, unbounded_channel},
	time::{Duration, sleep},
};
use versatiles::{
	config::{Config, StaticSourceConfig, TileSourceConfig},
	server::{TileServer, spawn_sighup_handler},
};
use versatiles_container::{DataLocation, DataSource, TilesRuntime};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true, verbatim_doc_comment)]
pub struct Subcommand {
	/// One or more tile containers to serve (path, URL, or data source expression).
	///
	/// Supported formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory.
	/// Remote URLs (https, http, sftp) work for *.versatiles and *.pmtiles;
	/// *.mbtiles, *.tar and directories have to be local.
	/// The URL path (/tiles/{id}/) is derived from the source name:
	///    e.g. "ukraine.versatiles" -> "/tiles/ukraine/..."
	/// Override the name using bracket notation:
	///    "[osm]tiles.versatiles"  or  "tiles.versatiles[osm]"
	/// Run `versatiles help source` for full syntax details.
	#[arg(verbatim_doc_comment)]
	pub tile_sources: Vec<String>,

	/// Path to a configuration file (YAML format) to configure the server, CORS, static and tile sources.
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
	/// Tar files can be compressed (.tar / .tar.gz / .tar.br / .tar.zst).
	/// If multiple static sources are defined, the first hit will be served.
	/// You can also add an optional url prefix like "[/assets/styles]styles.tar".
	#[arg(short = 's', long = "static", verbatim_doc_comment, display_order = 1)]
	pub static_content: Vec<String>,

	// The three switches below are `Option<bool>` so that leaving one out
	// keeps whatever the config file says. A bare `--follow-symlinks` means
	// `true`; `--follow-symlinks false` still overrides a config that set it.
	/// Shutdown server automatically after x milliseconds.
	#[arg(long, display_order = 4)]
	pub auto_shutdown: Option<u64>,

	/// use minimal recompression to reduce server response time
	#[arg(long, num_args = 0..=1, default_missing_value = "true", display_order = 2)]
	pub minimal_recompression: Option<bool>,

	/// disable API
	#[arg(long, num_args = 0..=1, default_missing_value = "true", display_order = 4)]
	pub disable_api: Option<bool>,

	/// serve files that symlinks in a static folder point to outside that folder
	///
	/// Off by default: a symlink inside a served folder works either way, but one
	/// pointing out of it is refused, so a folder cannot hand out files the
	/// operator did not put there. Turn it on to serve a tree that deliberately
	/// links elsewhere.
	#[arg(
		long,
		num_args = 0..=1,
		default_missing_value = "true",
		verbatim_doc_comment,
		display_order = 4
	)]
	pub follow_symlinks: Option<bool>,

	/// Cache-Control header sent with every tile, e.g. "no-cache"
	///
	/// Tile URLs are stable, so a client answers from its own cache when the
	/// tiles behind a mount change. The default of four weeks suits a public,
	/// CDN-fronted server; a preview or a test harness wants something shorter.
	#[arg(long, value_name = "header", display_order = 2, verbatim_doc_comment)]
	pub cache_control: Option<String>,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
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
	config
		.server
		.override_optional_follow_symlinks(&arguments.follow_symlinks);
	config.server.override_optional_cache_control(&arguments.cache_control);

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
	.map(|pat| Regex::new(pat).expect("valid regex literal"))
	.collect();

	let mut static_sources = arguments
		.static_content
		.iter()
		.map(|argument| {
			let capture = static_patterns
				.iter()
				.find(|p| p.is_match(argument))
				.expect("catch-all pattern always matches")
				.captures(argument)
				.expect("pattern matched; captures must succeed");

			let filename: &str = capture
				.name("filename")
				.expect("every pattern defines filename")
				.as_str();
			let prefix = capture.name("path").map(|m| m.as_str().to_string());

			Ok(StaticSourceConfig {
				src: DataLocation::parse(filename)?,
				prefix,
			})
		})
		.collect::<Result<Vec<StaticSourceConfig>>>()?;
	swap(&mut config.static_sources, &mut static_sources);
	config.static_sources.extend(static_sources);

	let mut server: TileServer = TileServer::from_config(config, runtime.clone()).await?;

	let mut list = server.url_mapping();
	list.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("url comparison is total"));
	for (url, source) in &list {
		log::info!("add tile source: {} <- {source}", url.join_as_string("*"));
	}

	server.start().await?;

	if let Some(config_path) = &arguments.config {
		spawn_sighup_handler(server.reload_handle(config_path.clone()));
	}

	let mut signals = shutdown_signals()?;

	tokio::select! {
		() = auto_shutdown(arguments.auto_shutdown) => {}
		Some(name) = signals.recv() => log::info!("received {name}, shutting down"),
	}

	// Stop accepting, and let the requests being answered finish (#282).
	// SIGTERM is what `docker stop`, systemd and Kubernetes send, so exiting
	// on the spot cut requests off on every deployment.
	tokio::select! {
		() = server.stop() => {}
		// Ctrl+C twice still gets out of a terminal at once. Not by returning:
		// dropping the runtime would wait for compression still running on
		// the blocking pool.
		Some(name) = signals.recv() => {
			log::warn!("received {name} again, exiting without waiting for open requests");
			std::process::exit(1);
		}
	}

	Ok(())
}

/// Resolves after `milliseconds`, or never when there is no `--auto-shutdown`.
async fn auto_shutdown(milliseconds: Option<u64>) {
	match milliseconds {
		Some(milliseconds) => sleep(Duration::from_millis(milliseconds)).await,
		None => std::future::pending().await,
	}
}

/// The names of the signals that ask the server to stop, as they arrive:
/// SIGTERM and SIGINT (Ctrl+C).
///
/// Registered once and kept. A registration replaces the signal's default
/// action for the rest of the process, so a second signal arrives here too
/// and is no longer fatal by itself — `run` listens for it explicitly.
#[cfg(unix)]
fn shutdown_signals() -> Result<UnboundedReceiver<&'static str>> {
	use tokio::signal::unix::{SignalKind, signal};

	let mut sigterm = signal(SignalKind::terminate()).context("registering the SIGTERM handler")?;
	let mut sigint = signal(SignalKind::interrupt()).context("registering the SIGINT handler")?;

	let (tx, rx) = unbounded_channel();
	tokio::spawn(async move {
		loop {
			let name = tokio::select! {
				_ = sigterm.recv() => "SIGTERM",
				_ = sigint.recv() => "SIGINT",
			};
			if tx.send(name).is_err() {
				return;
			}
		}
	});
	Ok(rx)
}

// Each `cfg` arm is a separate item and needs its own docs; see
// `spawn_sighup_handler`.
/// The names of the signals that ask the server to stop, as they arrive:
/// Ctrl+C, the only one this platform has.
#[cfg(not(unix))]
fn shutdown_signals() -> Result<UnboundedReceiver<&'static str>> {
	let (tx, rx) = unbounded_channel();
	tokio::spawn(async move {
		// An error means no handler could be registered: nothing to wait for.
		while tokio::signal::ctrl_c().await.is_ok() {
			if tx.send("Ctrl+C").is_err() {
				return;
			}
		}
		// Keep the channel open, or `recv` would report a shutdown.
		std::future::pending::<()>().await;
	});
	Ok(rx)
}

#[cfg(test)]
mod tests {
	use anyhow::Result;

	use crate::tests::run_command;

	/// The switches as a `serve` command line sets them; `None` means "keep the config".
	fn switches(args: &[&str]) -> Result<[Option<bool>; 3]> {
		use clap::Parser;

		let argv = ["versatiles", "serve"].iter().chain(args).copied();
		let crate::Commands::Serve(serve) = crate::Cli::try_parse_from(argv)?.command else {
			unreachable!("parsed a `serve` command line");
		};
		Ok([serve.minimal_recompression, serve.disable_api, serve.follow_symlinks])
	}

	#[test]
	fn a_switch_works_bare_with_a_value_or_left_out() -> Result<()> {
		let all = |value| [value; 3];
		assert_eq!(switches(&["a.versatiles"])?, all(None));
		assert_eq!(
			switches(&[
				"a.versatiles",
				"--minimal-recompression",
				"--disable-api",
				"--follow-symlinks"
			])?,
			all(Some(true))
		);
		assert_eq!(
			switches(&[
				"--minimal-recompression",
				"--disable-api",
				"--follow-symlinks",
				"-s",
				"public"
			])?,
			all(Some(true))
		);
		assert_eq!(
			switches(&[
				"--minimal-recompression",
				"true",
				"--disable-api=true",
				"--follow-symlinks",
				"true",
				"a.versatiles"
			])?,
			all(Some(true))
		);
		assert_eq!(
			switches(&[
				"--minimal-recompression",
				"false",
				"--disable-api",
				"false",
				"--follow-symlinks=false",
				"a.versatiles"
			])?,
			all(Some(false))
		);
		Ok(())
	}

	/// A bare switch directly before a tile source reads the source as its
	/// value. That is refused, loudly, rather than guessed at.
	#[test]
	fn a_bare_switch_does_not_swallow_a_tile_source() {
		let error = switches(&["--follow-symlinks", "a.versatiles"])
			.unwrap_err()
			.to_string();
		assert!(error.contains("invalid value 'a.versatiles'"), "{error}");
	}

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
			"50",
			"../testdata/berlin.mbtiles[test]",
		])?;
		Ok(())
	}

	#[test]
	fn test_remote() -> Result<()> {
		let server = crate::test_http_server::TestHttpServer::shared();
		run_command(vec![
			"versatiles",
			"serve",
			"-i",
			"127.0.0.1",
			"-p",
			"65002",
			"--auto-shutdown",
			"50",
			&format!("[test]{}", server.url("berlin.pmtiles")),
		])?;
		Ok(())
	}

	#[test]
	fn test_config() -> Result<()> {
		// Serve a config file whose tile sources are a local HTTP source (the
		// test server) and a local file — exercising config loading without
		// reaching out to download.versatiles.org.
		let server = crate::test_http_server::TestHttpServer::shared();
		let mbtiles = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/berlin.mbtiles");
		let temp_dir = assert_fs::TempDir::new()?;
		let config_path = temp_dir.path().join("config.yml");
		// Single-quoted YAML scalars so Windows backslash paths need no escaping.
		std::fs::write(
			&config_path,
			format!(
				"server:\n  ip: 127.0.0.1\ntiles:\n  - name: osm\n    src: '{}'\n  - name: berlin\n    src: '{}'\n",
				server.url("berlin.pmtiles"),
				mbtiles.display(),
			),
		)?;
		run_command(vec![
			"versatiles",
			"serve",
			"-c",
			config_path.to_str().expect("temp config path is valid UTF-8"),
			"-p",
			"65003",
			"--auto-shutdown",
			"50",
		])?;
		Ok(())
	}
}
