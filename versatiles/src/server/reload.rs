use super::sources::{ServerTileSource, StaticSource};
use crate::config::{Config, StaticSourceConfig, TileSourceConfig};
use anyhow::Result;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use std::{
	path::PathBuf,
	sync::{Arc, Mutex},
};
use versatiles_container::TilesRuntime;

pub struct ReloadHandle {
	pub(super) config_path: PathBuf,
	pub(super) tile_sources: Arc<DashMap<String, Arc<ServerTileSource>>>,
	pub(super) static_sources: Arc<ArcSwap<Vec<StaticSource>>>,
	pub(super) current_tile_configs: Arc<Mutex<Vec<TileSourceConfig>>>,
	pub(super) current_static_configs: Arc<Mutex<Vec<StaticSourceConfig>>>,
	pub(super) runtime: TilesRuntime,
}

impl ReloadHandle {
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
