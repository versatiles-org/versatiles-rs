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
		// The new config's value, not one captured at startup: a reload that
		// re-reads `follow_symlinks` and then ignores it would be a surprise.
		self
			.apply_static_source_diff(
				&new_config.static_sources,
				new_config.server.follow_symlinks.unwrap_or(false),
			)
			.await;
		Ok(())
	}

	/// Opens what is new or changed, swaps it in, and only then drops what is gone.
	///
	/// The order is the point (#283). A changed source is replaced by one
	/// `insert`, so there is no moment at which its name is missing and its
	/// requests are answered with a 404 — which a proxy in front would cache.
	/// Requests in flight keep their `Arc` to the source they started with.
	async fn apply_tile_source_diff(&self, new_configs: &[TileSourceConfig]) {
		let old_configs = self.current_tile_configs.lock().unwrap().clone();

		fn config_name(cfg: &TileSourceConfig) -> Option<String> {
			cfg.name
				.clone()
				.or_else(|| cfg.src.name().ok().map(ToString::to_string))
		}

		// What the server holds once this reload is through. Not simply
		// `new_configs`: a source that failed to open is left out, so that the
		// next reload sees it as still to do and tries again.
		let mut applied_configs: Vec<TileSourceConfig> = Vec::new();

		// Open sources that are new or changed, and swap each in as it is ready.
		for new in new_configs {
			let Some(new_name) = config_name(new) else {
				log::warn!("reload: skipping tile source with no resolvable name");
				continue;
			};
			if old_configs.contains(new) {
				applied_configs.push(new.clone());
				continue;
			}

			let source = match self.runtime.reader(new.src.clone()).await {
				Ok(reader) => ServerTileSource::from(reader, &new_name),
				Err(e) => Err(e.context("opening the source")),
			};
			match source {
				Ok(source) => {
					let replaced = self.tile_sources.insert(new_name.clone(), Arc::new(source)).is_some();
					let verb = if replaced { "replaced" } else { "added" };
					log::info!("reload: {verb} tile source '{new_name}'");
					applied_configs.push(new.clone());
				}
				Err(e) => {
					// The config the running source was opened from stays on record,
					// so the source keeps serving and still counts as changed.
					let previous = old_configs
						.iter()
						.rev()
						.find(|c| config_name(c).as_deref() == Some(&new_name));
					if let Some(previous) = previous {
						log::error!("reload: failed to replace tile source '{new_name}', keeping the previous one: {e:#}");
						applied_configs.push(previous.clone());
					} else {
						log::error!("reload: failed to add tile source '{new_name}': {e:#}");
					}
				}
			}
		}

		// Drop sources whose name is no longer in the config. By name, not by
		// config: a changed source has a new config under its old name, and was
		// replaced above.
		for old in &old_configs {
			let Some(old_name) = config_name(old) else {
				continue;
			};
			let still_wanted = new_configs.iter().any(|c| config_name(c).as_deref() == Some(&old_name));
			if !still_wanted && self.tile_sources.remove(&old_name).is_some() {
				log::info!("reload: removed tile source '{old_name}'");
			}
		}

		*self.current_tile_configs.lock().unwrap() = applied_configs;
	}

	async fn apply_static_source_diff(&self, new_configs: &[StaticSourceConfig], follow_symlinks: bool) {
		let old_configs = self.current_static_configs.lock().unwrap().clone();
		if old_configs == new_configs {
			return;
		}

		let mut new_sources: Vec<StaticSource> = Vec::new();
		for cfg in new_configs {
			let prefix = cfg.prefix.as_deref().unwrap_or("/");
			match StaticSource::from_location(&cfg.src, prefix, follow_symlinks).await {
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
///
/// Platforms without SIGHUP get the stub below instead, which warns and returns.
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

// Each `cfg` arm is a separate item and needs its own docs. The arm the host does
// not compile is invisible to `missing_docs`, so a Unix build cannot vouch for
// this one — it reached CI undocumented and broke every Windows job.
/// Warn that hot-reload is unavailable: this platform has no SIGHUP to listen for.
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
		assert_eq!(loaded_names(&handle), Vec::<String>::new());
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

	/// #283: the old source used to be removed before the new one was opened,
	/// so a failed open dropped the tileset.
	#[tokio::test]
	async fn a_changed_source_that_cannot_be_opened_keeps_the_old_one_serving() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_tile_source_diff(&[tile_config(Some("tiles"), "../testdata/berlin.mbtiles")])
			.await;
		let before = Arc::clone(&handle.tile_sources.get("tiles").unwrap());

		handle
			.apply_tile_source_diff(&[tile_config(Some("tiles"), "../testdata/does_not_exist.mbtiles")])
			.await;
		let after = Arc::clone(&handle.tile_sources.get("tiles").unwrap());
		assert!(Arc::ptr_eq(&before, &after), "the old source should still be serving");

		// Going back to the config that is being served is not a change.
		handle
			.apply_tile_source_diff(&[tile_config(Some("tiles"), "../testdata/berlin.mbtiles")])
			.await;
		let reverted = Arc::clone(&handle.tile_sources.get("tiles").unwrap());
		assert!(Arc::ptr_eq(&before, &reverted), "reverting must not reopen the source");
	}

	/// #283: a failed source used to be recorded as loaded, so every later
	/// reload skipped it and the tileset stayed missing until a restart.
	#[tokio::test]
	async fn a_source_that_failed_to_open_is_retried_on_the_next_reload() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let late = dir.path().join("late.mbtiles");
		let late_str = late.to_str().expect("temp path is valid UTF-8");
		let handle = handle(PathBuf::from("unused"));

		// A new source, and a change to a running one, both pointing at a file
		// that is not there yet.
		handle
			.apply_tile_source_diff(&[tile_config(Some("changed"), "../testdata/berlin.mbtiles")])
			.await;
		let before = Arc::clone(&handle.tile_sources.get("changed").unwrap());
		let config = [
			tile_config(Some("changed"), late_str),
			tile_config(Some("new"), late_str),
		];
		handle.apply_tile_source_diff(&config).await;
		assert_eq!(loaded_names(&handle), ["changed"]);

		// The file arrives; the same config is reloaded.
		std::fs::copy("../testdata/berlin.mbtiles", &late)?;
		handle.apply_tile_source_diff(&config).await;
		assert_eq!(loaded_names(&handle), ["changed", "new"]);
		let after = Arc::clone(&handle.tile_sources.get("changed").unwrap());
		assert!(
			!Arc::ptr_eq(&before, &after),
			"the retry should have replaced the source"
		);

		Ok(())
	}

	/// A source the config never named — one added through the API — is not
	/// the reload's to remove.
	#[tokio::test]
	async fn a_source_the_config_never_named_is_left_alone() -> Result<()> {
		let handle = handle(PathBuf::from("unused"));
		let reader = handle
			.runtime
			.reader(DataSource::parse("../testdata/berlin.mbtiles")?)
			.await?;
		handle.tile_sources.insert(
			"manual".to_string(),
			Arc::new(ServerTileSource::from(reader, "manual")?),
		);

		handle
			.apply_tile_source_diff(&[tile_config(Some("berlin"), "../testdata/berlin.mbtiles")])
			.await;
		handle.apply_tile_source_diff(&[]).await;
		assert_eq!(loaded_names(&handle), ["manual"]);

		Ok(())
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
		assert_eq!(loaded_names(&handle), Vec::<String>::new());

		// And on the next reload it is not mistaken for a source to remove
		// either — the removal pass skips it for the same reason.
		handle.apply_tile_source_diff(&[]).await;
		assert_eq!(loaded_names(&handle), Vec::<String>::new());
	}

	#[tokio::test]
	async fn static_sources_are_replaced_wholesale() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_static_source_diff(
				&[StaticSourceConfig {
					src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
					prefix: Some("/assets/".to_string()),
				}],
				false,
			)
			.await;
		assert_eq!(handle.static_sources.load().len(), 1);

		handle.apply_static_source_diff(&[], false).await;
		assert_eq!(handle.static_sources.load().len(), 0);
	}

	#[tokio::test]
	async fn an_unchanged_static_config_is_left_untouched() {
		let handle = handle(PathBuf::from("unused"));
		let config = [StaticSourceConfig {
			src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
			prefix: None,
		}];

		handle.apply_static_source_diff(&config, false).await;
		let first = handle.static_sources.load_full();

		handle.apply_static_source_diff(&config, false).await;
		let second = handle.static_sources.load_full();

		// The early return exists so an unchanged mount is not re-read from disk.
		assert!(Arc::ptr_eq(&first, &second), "unchanged static config was rebuilt");
	}

	#[tokio::test]
	async fn a_broken_static_source_does_not_stop_the_working_ones() {
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_static_source_diff(
				&[
					StaticSourceConfig {
						src: DataLocation::from(Path::new("../testdata/does_not_exist.tar")),
						prefix: None,
					},
					StaticSourceConfig {
						src: DataLocation::from(Path::new("../testdata/static.tar.gz")),
						prefix: Some("/ok/".to_string()),
					},
				],
				false,
			)
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
		assert_eq!(loaded_names(&handle), Vec::<String>::new());

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
