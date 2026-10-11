//! `versatiles serve -c …` reloads its config on SIGHUP, and applies what the
//! command line said on top of it again, as the start did.

#![cfg(unix)]

use std::{
	fs,
	path::Path,
	process::{Child, Command},
	time::Duration,
};

use crate::test_utilities::*;

fn write_config(path: &Path, follow_symlinks: bool, tiles: &[(&str, &str)]) {
	let tiles: Vec<String> = tiles
		.iter()
		.map(|(name, src)| format!("  - name: {name}\n    src: '{src}'\n"))
		.collect();
	let tiles = tiles.concat();
	fs::write(
		path,
		format!("server:\n  follow_symlinks: {follow_symlinks}\ntiles:\n{tiles}"),
	)
	.unwrap();
}

async fn status(host: &str, path: &str) -> u16 {
	reqwest::get(format!("{host}{path}")).await.unwrap().status().as_u16()
}

/// Sends SIGHUP and waits until `path` is served, which tells that the reload
/// is through.
async fn reload_and_wait_for(child: &Child, host: &str, path: &str) {
	let sent = Command::new("kill")
		.args(["-HUP", &child.id().to_string()])
		.status()
		.expect("failed to run kill");
	assert!(sent.success(), "kill -HUP failed");

	for _ in 0..100 {
		if status(host, path).await == 200 {
			return;
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	panic!("{path} was not served within 5 s of the SIGHUP");
}

/// A reload used to read the config file alone, so a tile source and a static
/// source given on the command line were dropped by the first SIGHUP.
#[tokio::test]
async fn e2e_reload_keeps_the_sources_of_the_command_line() {
	let dir = tempfile::tempdir().unwrap();
	let config = dir.path().join("config.yml");
	let mbtiles = get_testdata("berlin.mbtiles");
	write_config(&config, false, &[("from_config", &mbtiles)]);

	let tile_source = format!("[from_cli]{}", get_testdata("berlin.pmtiles"));
	let static_source = format!("[/from_cli/]{}", get_testdata("static.tar.gz"));
	let (host, mut child) = spawn_server(
		&["-c", config.to_str().unwrap(), "-s", &static_source, &tile_source],
		"/tiles/index.json",
	)
	.await;

	write_config(&config, false, &[("from_config", &mbtiles), ("added", &mbtiles)]);
	reload_and_wait_for(&child, &host, "/tiles/added/tiles.json").await;

	let statuses = [
		status(&host, "/tiles/from_config/tiles.json").await,
		status(&host, "/tiles/from_cli/tiles.json").await,
		status(&host, "/from_cli/index.html").await,
	];
	let _ = child.kill();
	let _ = child.wait();
	assert_eq!(statuses, [200, 200, 200]);
}

/// The override with a security side: `--follow-symlinks false` over a config
/// that says `true` has to hold after a reload too, or a symlink pointing out
/// of the served folder becomes readable.
#[tokio::test]
async fn e2e_reload_keeps_the_settings_of_the_command_line() {
	let dir = tempfile::tempdir().unwrap();
	let config = dir.path().join("config.yml");
	let mbtiles = get_testdata("berlin.mbtiles");
	write_config(&config, true, &[("berlin", &mbtiles)]);

	let secret = dir.path().join("secret.txt");
	fs::write(&secret, "outside the served folder").unwrap();
	let public = dir.path().join("public");
	fs::create_dir(&public).unwrap();
	fs::write(public.join("inside.txt"), "inside").unwrap();
	std::os::unix::fs::symlink(&secret, public.join("link.txt")).unwrap();

	let (host, mut child) = spawn_server(
		&[
			"-c",
			config.to_str().unwrap(),
			"--follow-symlinks=false",
			"-s",
			public.to_str().unwrap(),
		],
		"/tiles/index.json",
	)
	.await;
	let before = [status(&host, "/inside.txt").await, status(&host, "/link.txt").await];

	write_config(&config, true, &[("berlin", &mbtiles), ("added", &mbtiles)]);
	reload_and_wait_for(&child, &host, "/tiles/added/tiles.json").await;
	let after = [status(&host, "/inside.txt").await, status(&host, "/link.txt").await];

	let _ = child.kill();
	let _ = child.wait();
	assert_eq!(before, [200, 404]);
	assert_eq!(after, [200, 404], "the reload dropped --follow-symlinks=false");
}
