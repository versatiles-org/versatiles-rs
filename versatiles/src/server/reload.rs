use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
};

use anyhow::Result;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use versatiles_container::TilesRuntime;

use super::sources::{ServerTileSource, StaticSource};
use crate::config::{Config, StaticSourceConfig, TileSourceConfig};

/// Re-reads the config file and applies the difference to a running server.
///
/// Sources that did not change keep their open containers and caches, so a
/// reload costs only what actually changed.
pub struct ReloadHandle {
	pub(super) config_path: PathBuf,
	pub(super) tile_sources: Arc<DashMap<String, Arc<ServerTileSource>>>,
	pub(super) static_sources: Arc<ArcSwap<Vec<StaticSource>>>,
	pub(super) current_tile_configs: Arc<Mutex<Vec<TileSourceConfig>>>,
	pub(super) current_static_configs: Arc<Mutex<Vec<StaticSourceConfig>>>,
	pub(super) runtime: TilesRuntime,
}

impl ReloadHandle {
	/// Re-reads the config and adds, drops or replaces sources to match it.
	pub async fn reload(&self) -> Result<()> {
		let new_config = Config::from_path(&self.config_path)?;
		self.apply_tile_source_diff(&new_config.tile_sources).await;
		self.apply_static_source_diff(&new_config.static_sources).await;
		Ok(())
	}

	async fn apply_tile_source_diff(&self, new_configs: &[TileSourceConfig]) {
		let old_configs = self.current_tile_configs.lock().unwrap().clone();

		fn config_name(cfg: &TileSourceConfig) -> Option<String> {
			cfg.name
				.clone()
				.or_else(|| cfg.src.name().ok().map(ToString::to_string))
		}

		// Remove sources that disappeared or changed.
		for old in &old_configs {
			let Some(old_name) = config_name(old) else {
				continue;
			};
			let matches = new_configs
				.iter()
				.any(|c| config_name(c).as_deref() == Some(&old_name) && c == old);
			if !matches {
				self.tile_sources.remove(&old_name);
				log::info!("reload: removed tile source '{old_name}'");
			}
		}

		// Add sources that are new or changed.
		for new in new_configs {
			let Some(new_name) = config_name(new) else {
				log::warn!("reload: skipping tile source with no resolvable name");
				continue;
			};
			let already_loaded = old_configs
				.iter()
				.any(|c| config_name(c).as_deref() == Some(&new_name) && c == new);
			if already_loaded {
				continue;
			}
			match self.runtime.reader(new.src.clone()).await {
				Ok(reader) => match ServerTileSource::from(reader, &new_name) {
					Ok(source) => {
						self.tile_sources.insert(new_name.clone(), Arc::new(source));
						log::info!("reload: added tile source '{new_name}'");
					}
					Err(e) => log::error!("reload: failed to build tile source '{new_name}': {e:#}"),
				},
				Err(e) => log::error!("reload: failed to open tile source '{new_name}': {e:#}"),
			}
		}

		*self.current_tile_configs.lock().unwrap() = new_configs.to_vec();
	}

	async fn apply_static_source_diff(&self, new_configs: &[StaticSourceConfig]) {
		let old_configs = self.current_static_configs.lock().unwrap().clone();
		if old_configs == new_configs {
			return;
		}

		let mut new_sources: Vec<StaticSource> = Vec::new();
		for cfg in new_configs {
			let prefix = cfg.prefix.as_deref().unwrap_or("/");
			match StaticSource::from_location(&cfg.src, prefix).await {
				Ok(source) => new_sources.push(source),
				Err(e) => log::error!("reload: failed to build static source at '{prefix}': {e:#}"),
			}
		}

		self.static_sources.store(Arc::new(new_sources));
		*self.current_static_configs.lock().unwrap() = new_configs.to_vec();
		log::info!("reload: static sources updated");
	}
}

/// Spawn a background task that reloads the config from `handle.config_path` on every SIGHUP.
/// No-op on non-Unix platforms.
#[cfg(unix)]
pub fn spawn_sighup_handler(handle: ReloadHandle) {
	use tokio::signal::unix::{SignalKind, signal};

	tokio::spawn(async move {
		let mut sighup = match signal(SignalKind::hangup()) {
			Ok(s) => s,
			Err(e) => {
				log::error!("failed to register SIGHUP handler: {e}");
				return;
			}
		};

		loop {
			sighup.recv().await;
			log::info!("received SIGHUP — reloading config from {:?}", handle.config_path);
			if let Err(e) = handle.reload().await {
				log::error!("config reload failed: {e:#}");
			} else {
				log::info!("config reload complete");
			}
		}
	});
}

#[cfg(not(unix))]
pub fn spawn_sighup_handler(_handle: ReloadHandle) {
	log::warn!("SIGHUP hot-reload is not supported on this platform");
}

#[cfg(test)]
mod tests {
	use std::path::Path;

	use versatiles_container::{DataLocation, DataSource};

	use super::*;

	/// A handle wired to `path`, holding nothing yet.
	fn handle(config_path: PathBuf) -> ReloadHandle {
		ReloadHandle {
			config_path,
			tile_sources: Arc::new(DashMap::new()),
			static_sources: Arc::new(ArcSwap::from_pointee(Vec::new())),
			current_tile_configs: Arc::new(Mutex::new(Vec::new())),
			current_static_configs: Arc::new(Mutex::new(Vec::new())),
			runtime: TilesRuntime::new_silent(),
		}
	}

	fn tile_config(name: Option<&str>, src: &str) -> TileSourceConfig {
		TileSourceConfig {
			name: name.map(ToString::to_string),
			src: DataSource::parse(src).unwrap(),
		}
	}

	/// Write a config naming one tile source at `src`.
	///
	/// Single-quoted, which is the whole reason this is a function: an absolute
	/// Windows path is full of backslashes, and YAML reads `\U` in a
	/// double-quoted scalar as an escape sequence. A single-quoted scalar has no
	/// escapes at all.
	fn write_tiles_config(config_path: &Path, name: &str, src: &Path) -> Result<()> {
		std::fs::write(
			config_path,
			format!("tiles:\n  - name: {name}\n    src: '{}'\n", src.display()),
		)?;
		Ok(())
	}

	fn loaded_names(handle: &ReloadHandle) -> Vec<String> {
		let mut names: Vec<String> = handle.tile_sources.iter().map(|e| e.key().clone()).collect();
		names.sort();
		names
	}

	#[tokio::test]
	async fn a_new_source_is_opened_and_a_vanished_one_is_dropped() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_tile_source_diff(&[tile_config(Some("berlin"), "../testdata/berlin.mbtiles")])
			.await;
		assert_eq!(loaded_names(&handle), ["berlin"]);

		// The same source under a second name: the first stays, the second appears.
		handle
			.apply_tile_source_diff(&[
				tile_config(Some("berlin"), "../testdata/berlin.mbtiles"),
				tile_config(Some("also_berlin"), "../testdata/berlin.pmtiles"),
			])
			.await;
		assert_eq!(loaded_names(&handle), ["also_berlin", "berlin"]);

		// Dropped from the config, so dropped from the server.
		handle.apply_tile_source_diff(&[]).await;
		assert!(loaded_names(&handle).is_empty());
	}

	#[tokio::test]
	async fn an_unchanged_source_is_not_reopened() {
		let handle = handle(PathBuf::from("unused"));
		let config = [tile_config(Some("berlin"), "../testdata/berlin.mbtiles")];

		handle.apply_tile_source_diff(&config).await;
		let first = Arc::clone(&handle.tile_sources.get("berlin").unwrap());

		handle.apply_tile_source_diff(&config).await;
		let second = Arc::clone(&handle.tile_sources.get("berlin").unwrap());

		// Same allocation: reload must not disturb a source whose config did not
		// change, or every reload would drop caches and reopen every file.
		assert!(Arc::ptr_eq(&first, &second), "unchanged source was reopened");
	}

	#[tokio::test]
	async fn a_source_whose_path_changed_under_the_same_name_is_replaced() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_tile_source_diff(&[tile_config(Some("tiles"), "../testdata/berlin.mbtiles")])
			.await;
		let before = Arc::clone(&handle.tile_sources.get("tiles").unwrap());

		handle
			.apply_tile_source_diff(&[tile_config(Some("tiles"), "../testdata/berlin.pmtiles")])
			.await;
		let after = Arc::clone(&handle.tile_sources.get("tiles").unwrap());

		assert!(!Arc::ptr_eq(&before, &after), "same name, new path: expected a reopen");
		let description = after.source_description();
		assert!(description.contains("pmtiles"), "{description}");
	}

	#[tokio::test]
	async fn a_source_that_cannot_be_opened_leaves_the_others_alone() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_tile_source_diff(&[
				tile_config(Some("good"), "../testdata/berlin.mbtiles"),
				tile_config(Some("gone"), "../testdata/does_not_exist.mbtiles"),
			])
			.await;

		// The broken one is logged and skipped; a reload must not take the server
		// down or discard the sources that did open.
		assert_eq!(loaded_names(&handle), ["good"]);
	}

	#[tokio::test]
	async fn a_source_with_no_resolvable_name_is_skipped() {
		// A DataSource whose location yields no basename has nothing to be
		// served under, so it is dropped with a warning rather than mounted at
		// an empty path.
		let mut config = tile_config(None, "../testdata/berlin.mbtiles");
		config.src = DataSource::parse("/").unwrap();
		assert!(config.src.name().is_err(), "this source should have no name");

		let handle = handle(PathBuf::from("unused"));
		handle.apply_tile_source_diff(&[config.clone()]).await;
		assert!(loaded_names(&handle).is_empty());

		// And on the next reload it is not mistaken for a source to remove
		// either — the removal pass skips it for the same reason.
		handle.apply_tile_source_diff(&[]).await;
		assert!(loaded_names(&handle).is_empty());
	}

	#[tokio::test]
	async fn static_sources_are_replaced_wholesale() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_static_source_diff(&[StaticSourceConfig {
				src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
				prefix: Some("/assets/".to_string()),
			}])
			.await;
		assert_eq!(handle.static_sources.load().len(), 1);

		handle.apply_static_source_diff(&[]).await;
		assert_eq!(handle.static_sources.load().len(), 0);
	}

	#[tokio::test]
	async fn an_unchanged_static_config_is_left_untouched() {
		let handle = handle(PathBuf::from("unused"));
		let config = [StaticSourceConfig {
			src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
			prefix: None,
		}];

		handle.apply_static_source_diff(&config).await;
		let first = handle.static_sources.load_full();

		handle.apply_static_source_diff(&config).await;
		let second = handle.static_sources.load_full();

		// The early return exists so an unchanged mount is not re-read from disk.
		assert!(Arc::ptr_eq(&first, &second), "unchanged static config was rebuilt");
	}

	#[tokio::test]
	async fn a_broken_static_source_does_not_stop_the_working_ones() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_static_source_diff(&[
				StaticSourceConfig {
					src: DataLocation::from(Path::new("../testdata/does_not_exist.tar")),
					prefix: None,
				},
				StaticSourceConfig {
					src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
					prefix: Some("/ok/".to_string()),
				},
			])
			.await;

		assert_eq!(handle.static_sources.load().len(), 1);
	}

	#[tokio::test]
	async fn reload_reads_the_config_file_from_disk() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let config_path = dir.path().join("config.yml");
		let testdata = std::fs::canonicalize("../testdata")?;

		write_tiles_config(&config_path, "berlin", &testdata.join("berlin.mbtiles"))?;

		let handle = handle(config_path.clone());
		handle.reload().await?;
		assert_eq!(loaded_names(&handle), ["berlin"]);

		// Rewriting the file and reloading again picks the change up.
		std::fs::write(&config_path, "tiles: []\n")?;
		handle.reload().await?;
		assert!(loaded_names(&handle).is_empty());

		Ok(())
	}

	#[tokio::test]
	async fn reload_reports_a_config_it_cannot_read() {
		let handle = handle(PathBuf::from("../testdata/no_such_config.yml"));
		assert!(handle.reload().await.is_err());
	}

	#[cfg(unix)]
	#[tokio::test]
	async fn a_sighup_reloads_the_config() -> Result<()> {
		use tokio::signal::unix::{SignalKind, signal};

		// Registering here first is what makes this safe to run: SIGHUP's default
		// action is to kill the process, and until *someone* has a handler
		// installed a stray signal would take the whole test binary with it.
		// Tokio's registration is process-wide, so this closes the window before
		// the handler under test opens its own.
		let _guard = signal(SignalKind::hangup())?;

		let dir = tempfile::tempdir()?;
		let config_path = dir.path().join("config.yml");
		let testdata = std::fs::canonicalize("../testdata")?;
		std::fs::write(&config_path, "tiles: []\n")?;

		let handle = handle(config_path.clone());
		let sources = Arc::clone(&handle.tile_sources);
		spawn_sighup_handler(handle);
		// Let the spawned task reach `sighup.recv()`.
		tokio::time::sleep(std::time::Duration::from_millis(100)).await;

		// A source appears in the file that the server has never seen.
		write_tiles_config(&config_path, "berlin", &testdata.join("berlin.mbtiles"))?;

		std::process::Command::new("kill")
			.args(["-HUP", &std::process::id().to_string()])
			.status()?;

		// The reload is asynchronous; wait for it rather than assuming a delay.
		for _ in 0..100 {
			if sources.contains_key("berlin") {
				return Ok(());
			}
			tokio::time::sleep(std::time::Duration::from_millis(50)).await;
		}
		panic!("SIGHUP did not reload the config within 5 s");
	}
}
