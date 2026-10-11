use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use versatiles_container::TilesRuntime;

use super::{
	Url,
	sources::{ServerTileSource, StaticSource},
};
use crate::config::{Config, StaticSourceConfig, TileSourceConfig};

/// A static source the config named, and what it was built from.
#[derive(Clone)]
pub(super) struct MountedStatic {
	pub(super) config: StaticSourceConfig,
	/// Part of what the source was built from: a folder built to follow
	/// symlinks has to be rebuilt when the setting changes.
	pub(super) follow_symlinks: bool,
	pub(super) source: StaticSource,
}

/// Loads the config and applies the difference to a running server.
///
/// Sources that did not change keep their open containers and caches, so a
/// reload costs only what actually changed.
pub struct ReloadHandle {
	/// Assembles the config the way it was assembled at startup.
	pub(super) load_config: Box<dyn Fn() -> Result<Config> + Send + Sync>,
	pub(super) tile_sources: Arc<DashMap<String, Arc<ServerTileSource>>>,
	pub(super) static_sources: Arc<ArcSwap<Vec<StaticSource>>>,
	pub(super) current_tile_configs: Arc<Mutex<Vec<TileSourceConfig>>>,
	pub(super) mounted_static: Arc<Mutex<Vec<MountedStatic>>>,
	pub(super) runtime: TilesRuntime,
}

/// The name a tile source is served under, if it has one.
fn config_name(cfg: &TileSourceConfig) -> Option<String> {
	cfg.name
		.clone()
		.or_else(|| cfg.src.name().ok().map(ToString::to_string))
}

/// Refuses tile sources that would be served under the same or overlapping URLs.
///
/// The start refuses them too. Applied one by one, the later of two sources
/// with one name would silently replace the earlier.
fn check_tile_source_names(configs: &[TileSourceConfig]) -> Result<()> {
	let mut seen: Vec<(String, Url)> = Vec::new();
	for cfg in configs {
		let Some(name) = config_name(cfg) else {
			continue;
		};
		let prefix = ServerTileSource::prefix_for(&name);
		for (other_name, other_prefix) in &seen {
			if *other_name == name {
				bail!("tile source '{name}' is defined more than once");
			}
			if other_prefix.starts_with(&prefix) || prefix.starts_with(other_prefix) {
				bail!(
					"URL prefix collision: tile source '{name}' ({prefix}) conflicts with '{other_name}' ({other_prefix})"
				);
			}
		}
		seen.push((name, prefix));
	}
	Ok(())
}

impl ReloadHandle {
	/// Loads the config and adds, drops or replaces sources to match it.
	///
	/// A config that cannot be loaded, or that names two tile sources for the
	/// same URLs, is refused as a whole: the server keeps running as it is.
	pub async fn reload(&self) -> Result<()> {
		let new_config = (self.load_config)()?;
		check_tile_source_names(&new_config.tile_sources).context("the config is refused, nothing was changed")?;

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

	/// Builds what is new or changed, and swaps the whole list in at once.
	///
	/// Handled like tile sources: a source that did not change is not rebuilt,
	/// and one that cannot be built leaves the source it would have replaced
	/// in place, where it is clear which one that is. What failed is not
	/// recorded as mounted, so the next reload tries again.
	async fn apply_static_source_diff(&self, new_configs: &[StaticSourceConfig], follow_symlinks: bool) {
		fn prefix_of(cfg: &StaticSourceConfig) -> &str {
			cfg.prefix.as_deref().unwrap_or("/")
		}

		let old = self.mounted_static.lock().unwrap().clone();
		let mut taken = vec![false; old.len()];

		// Settle what stays before building anything, so that a source that
		// fails below cannot claim an old one a later entry still uses.
		let reused: Vec<Option<usize>> = new_configs
			.iter()
			.map(|cfg| {
				let index = (0..old.len())
					.find(|&i| !taken[i] && old[i].config == *cfg && old[i].follow_symlinks == follow_symlinks)?;
				taken[index] = true;
				Some(index)
			})
			.collect();

		// How many sources at a prefix are not carried over, and how many of the
		// new config are not among the old ones: settled now, before a kept
		// source changes the count.
		let leaving_at: Vec<&str> = (0..old.len())
			.filter(|&i| !taken[i])
			.map(|i| prefix_of(&old[i].config))
			.collect();
		let arriving_at: Vec<&str> = new_configs
			.iter()
			.zip(&reused)
			.filter(|(_, reused)| reused.is_none())
			.map(|(cfg, _)| prefix_of(cfg))
			.collect();
		let leaving = |prefix: &str| leaving_at.iter().filter(|p| **p == prefix).count();
		let arriving = |prefix: &str| arriving_at.iter().filter(|p| **p == prefix).count();

		let mut mounted: Vec<MountedStatic> = Vec::new();
		let mut changed = false;
		for (cfg, reused) in new_configs.iter().zip(reused) {
			if let Some(index) = reused {
				changed |= index != mounted.len();
				mounted.push(old[index].clone());
				continue;
			}

			changed = true;
			let prefix = prefix_of(cfg);
			match StaticSource::from_location(&cfg.src, prefix, follow_symlinks).await {
				Ok(source) => {
					log::info!("reload: mounted static source at '{prefix}'");
					mounted.push(MountedStatic {
						config: cfg.clone(),
						follow_symlinks,
						source,
					});
				}
				Err(e) => {
					// Static sources have no name; the URL prefix is what tells
					// which one this was meant to replace. It only tells when
					// one source left and one arrived at that prefix: with
					// several sharing it, any choice could keep a source the
					// config dropped on purpose, in front of the others.
					//
					// A source that follows symlinks is not kept under a config
					// that says not to.
					let previous = (0..old.len())
						.find(|&i| !taken[i] && prefix_of(&old[i].config) == prefix)
						.filter(|_| leaving(prefix) == 1 && arriving(prefix) == 1)
						.filter(|&i| follow_symlinks || !old[i].follow_symlinks);
					if let Some(index) = previous {
						log::error!(
							"reload: failed to replace the static source at '{prefix}', keeping the previous one: {e:#}"
						);
						taken[index] = true;
						mounted.push(old[index].clone());
					} else {
						log::error!("reload: failed to mount a static source at '{prefix}': {e:#}");
					}
				}
			}
		}

		for (index, old) in old.iter().enumerate() {
			if !taken[index] {
				changed = true;
				log::info!("reload: removed static source at '{}'", prefix_of(&old.config));
			}
		}

		if !changed {
			return;
		}

		let sources: Vec<StaticSource> = mounted.iter().map(|m| m.source.clone()).collect();
		self.static_sources.store(Arc::new(sources));
		*self.mounted_static.lock().unwrap() = mounted;
	}
}

/// Spawn a background task that reloads the config on every SIGHUP.
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
			log::info!("received SIGHUP — reloading config");
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
	use std::path::{Path, PathBuf};

	use versatiles_container::{DataLocation, DataSource};

	use super::*;

	/// A handle wired to `path`, holding nothing yet.
	fn handle(config_path: PathBuf) -> ReloadHandle {
		ReloadHandle {
			load_config: Box::new(move || Config::from_path(&config_path)),
			tile_sources: Arc::new(DashMap::new()),
			static_sources: Arc::new(ArcSwap::from_pointee(Vec::new())),
			current_tile_configs: Arc::new(Mutex::new(Vec::new())),
			mounted_static: Arc::new(Mutex::new(Vec::new())),
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
	async fn a_static_source_is_mounted_and_a_vanished_one_is_removed() {
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

	fn static_config(src: &Path, prefix: &str) -> StaticSourceConfig {
		StaticSourceConfig {
			src: DataLocation::from(src),
			prefix: Some(prefix.to_string()),
		}
	}

	fn mounted_configs(handle: &ReloadHandle) -> Vec<StaticSourceConfig> {
		let mounted = handle.mounted_static.lock().unwrap();
		mounted.iter().map(|m| m.config.clone()).collect()
	}

	async fn is_served(handle: &ReloadHandle, path: &str) -> bool {
		let accept = versatiles_core::compression::TargetCompression::from_none();
		for source in handle.static_sources.load().iter() {
			if source.get_data(&Url::from(path), &accept).await.is_some() {
				return true;
			}
		}
		false
	}

	/// O6: a static source that failed to build used to drop its mount, and
	/// was recorded as loaded, so no later reload tried it again.
	#[tokio::test]
	async fn a_changed_static_source_that_cannot_be_built_keeps_the_old_one_and_is_retried() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let late = dir.path().join("late.tar.gz");
		let handle = handle(PathBuf::from("unused"));

		let running = static_config(Path::new("../testdata/static.tar.gz"), "/assets/");
		handle
			.apply_static_source_diff(std::slice::from_ref(&running), false)
			.await;
		assert!(is_served(&handle, "/assets/index.html").await);

		// The mount now points at a file that is not there yet, and a second
		// mount is new and broken as well.
		let config = [static_config(&late, "/assets/"), static_config(&late, "/more/")];
		handle.apply_static_source_diff(&config, false).await;
		assert!(is_served(&handle, "/assets/index.html").await, "the old mount is gone");
		assert!(!is_served(&handle, "/more/index.html").await);
		assert_eq!(mounted_configs(&handle), [running]);

		// The file arrives; the same config is reloaded.
		std::fs::copy("../testdata/static.tar.gz", &late)?;
		handle.apply_static_source_diff(&config, false).await;
		assert!(is_served(&handle, "/assets/index.html").await);
		assert!(is_served(&handle, "/more/index.html").await);
		assert_eq!(mounted_configs(&handle), config);

		Ok(())
	}

	/// Several static sources may share a prefix; the first with the file
	/// answers. A failed build then does not say which old source it was meant
	/// to replace, and keeping the first one left a source the config had
	/// dropped mounted in front of the others.
	#[tokio::test]
	async fn a_failed_static_source_keeps_no_old_one_where_several_share_the_prefix() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let (a, b) = (dir.path().join("a"), dir.path().join("b"));
		for (folder, name) in [(&a, "a.txt"), (&b, "b.txt")] {
			std::fs::create_dir(folder)?;
			std::fs::write(folder.join(name), name)?;
		}
		let missing = dir.path().join("missing");
		let handle = handle(PathBuf::from("unused"));

		// One leaves, two arrive, one of them broken.
		handle.apply_static_source_diff(&[static_config(&a, "/")], false).await;
		handle
			.apply_static_source_diff(&[static_config(&missing, "/"), static_config(&b, "/")], false)
			.await;
		assert!(!is_served(&handle, "/a.txt").await, "a dropped source is still mounted");
		assert!(is_served(&handle, "/b.txt").await);

		// Two leave, one broken one arrives.
		handle
			.apply_static_source_diff(&[static_config(&a, "/"), static_config(&b, "/")], false)
			.await;
		handle
			.apply_static_source_diff(&[static_config(&missing, "/")], false)
			.await;
		assert!(!is_served(&handle, "/a.txt").await);
		assert!(!is_served(&handle, "/b.txt").await);
		Ok(())
	}

	/// A folder that follows symlinks is not kept in place of one that could
	/// not be built, once the config says not to follow them.
	#[tokio::test]
	async fn a_failed_static_source_does_not_keep_one_that_follows_symlinks_against_the_config() -> Result<()> {
		let dir = tempfile::tempdir()?;
		let public = dir.path().join("public");
		std::fs::create_dir(&public)?;
		std::fs::write(public.join("inside.txt"), "inside")?;
		let missing = dir.path().join("missing");
		let handle = handle(PathBuf::from("unused"));

		handle
			.apply_static_source_diff(&[static_config(&public, "/")], true)
			.await;
		assert!(is_served(&handle, "/inside.txt").await);

		handle
			.apply_static_source_diff(&[static_config(&missing, "/")], false)
			.await;
		assert!(!is_served(&handle, "/inside.txt").await);

		// The other way round, the stricter source may stay.
		handle
			.apply_static_source_diff(&[static_config(&public, "/")], false)
			.await;
		handle
			.apply_static_source_diff(&[static_config(&missing, "/")], true)
			.await;
		assert!(is_served(&handle, "/inside.txt").await);
		Ok(())
	}

	#[tokio::test]
	async fn a_changed_follow_symlinks_setting_rebuilds_the_static_sources() {
		let handle = handle(PathBuf::from("unused"));
		let config = [static_config(Path::new("../testdata"), "/")];

		handle.apply_static_source_diff(&config, true).await;
		let first = handle.static_sources.load_full();

		handle.apply_static_source_diff(&config, false).await;
		let second = handle.static_sources.load_full();

		assert!(!Arc::ptr_eq(&first, &second), "the folder still follows symlinks");
		assert!(!handle.mounted_static.lock().unwrap()[0].follow_symlinks);
	}

	/// O5: startup refuses these, and a reload used to take them: the later
	/// source replaced the earlier under its name.
	#[rstest::rstest]
	#[case::the_same_name("tiles", "tiles", "tile source 'tiles' is defined more than once")]
	#[case::overlapping_urls("a", "a/b", "URL prefix collision")]
	#[tokio::test]
	async fn a_config_with_colliding_names_is_refused_as_a_whole(
		#[case] first: &str,
		#[case] second: &str,
		#[case] expected: &str,
	) -> Result<()> {
		let mut handle = handle(PathBuf::from("unused"));
		handle
			.apply_tile_source_diff(&[tile_config(Some("running"), "../testdata/berlin.mbtiles")])
			.await;
		let before = Arc::clone(&handle.tile_sources.get("running").unwrap());

		let colliding = vec![
			tile_config(Some(first), "../testdata/berlin.mbtiles"),
			tile_config(Some(second), "../testdata/berlin.pmtiles"),
		];
		handle.load_config = Box::new(move || {
			Ok(Config {
				tile_sources: colliding.clone(),
				static_sources: vec![static_config(Path::new("../testdata/static.tar.gz"), "/")],
				..Config::default()
			})
		});

		let error = format!("{:#}", handle.reload().await.unwrap_err());
		assert!(error.contains(expected), "{error}");

		// Nothing was applied: not the tile sources, not the static one.
		assert_eq!(loaded_names(&handle), ["running"]);
		let after = Arc::clone(&handle.tile_sources.get("running").unwrap());
		assert!(Arc::ptr_eq(&before, &after));
		assert_eq!(handle.static_sources.load().len(), 0);

		Ok(())
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
