use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use versatiles_container::{SharedTileSource, TilesRuntime};
use versatiles_core::{TileBBox, TileBBoxPyramid};

/// Metadata stored per source container (kept in memory even when reader is closed).
#[derive(Debug)]
pub struct SourceInfo {
	pub path: String,
	pub pyramid: TileBBoxPyramid,
}

/// LRU cache of open container readers with a maximum capacity.
///
/// Stores lightweight metadata (`SourceInfo`) for all sources, but only keeps
/// up to `max_open` readers open at a time. When a new reader needs to be
/// opened and the cache is full, the least recently used reader is evicted.
pub struct ReaderCache {
	sources: Vec<SourceInfo>,
	readers: HashMap<usize, SharedTileSource>,
	usage_order: VecDeque<usize>,
	max_open: usize,
	runtime: TilesRuntime,
}

impl std::fmt::Debug for ReaderCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ReaderCache")
			.field("sources", &self.sources)
			.field("open_readers", &self.readers.len())
			.field("max_open", &self.max_open)
			.finish_non_exhaustive()
	}
}

impl ReaderCache {
	pub fn new(sources: Vec<SourceInfo>, max_open: usize, runtime: TilesRuntime) -> Self {
		Self {
			sources,
			readers: HashMap::new(),
			usage_order: VecDeque::new(),
			max_open,
			runtime,
		}
	}

	/// Returns indices of sources whose pyramids overlap the given bbox.
	fn overlapping_sources(&self, bbox: &TileBBox) -> Vec<usize> {
		self.sources
			.iter()
			.enumerate()
			.filter(|(_, info)| info.pyramid.overlaps_bbox(bbox))
			.map(|(i, _)| i)
			.collect()
	}

	/// Get or open a reader for the source at `index`, evicting LRU readers if at capacity.
	async fn get_reader(&mut self, index: usize) -> Result<SharedTileSource> {
		if let Some(reader) = self.readers.get(&index) {
			// Move to back of usage_order (most recently used)
			self.usage_order.retain(|&i| i != index);
			self.usage_order.push_back(index);
			return Ok(Arc::clone(reader));
		}

		// Evict LRU readers if at capacity
		while self.readers.len() >= self.max_open {
			if let Some(evict_idx) = self.usage_order.pop_front() {
				self.readers.remove(&evict_idx);
				log::trace!("evicted reader for source {evict_idx}");
			} else {
				break;
			}
		}

		// Open a new reader
		let path = &self.sources[index].path;
		log::trace!("opening reader for source {index}: {path}");
		let reader = self
			.runtime
			.get_reader_from_str(path)
			.await
			.with_context(|| format!("Failed to reopen container: {path}"))?;
		self.readers.insert(index, Arc::clone(&reader));
		self.usage_order.push_back(index);
		Ok(reader)
	}

	/// Get readers for all sources that overlap the given bbox.
	pub async fn get_overlapping_readers(&mut self, bbox: &TileBBox) -> Result<Vec<SharedTileSource>> {
		let indices = self.overlapping_sources(bbox);
		let mut readers = Vec::with_capacity(indices.len());
		for idx in indices {
			readers.push(self.get_reader(idx).await?);
		}
		Ok(readers)
	}
}
