//! Specialized cache for tile traversal reordering.
//!
//! `TraversalCache<V>` provides a simple append-and-take cache optimized for the
//! tile traversal use case, where tiles are temporarily cached during Push phases
//! and retrieved during Pop phases.
//!
//! Unlike the generic `CacheMap`, this cache:
//! - Uses `usize` keys directly (no string conversion)
//! - Only provides `append` and `take` operations
//! - Uses simpler filenames for disk storage

use crate::cache::{cache_type::CacheType, traits::CacheValue};
use anyhow::Result;
use dashmap::DashMap;
use futures::{Stream, StreamExt, stream::BoxStream};
use std::{
	fmt::Debug,
	fs::{File, create_dir_all, read_dir, remove_dir_all},
	io::{BufWriter, Cursor, Read, Write},
	marker::PhantomData,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
use versatiles_derive::context;

/// A cache for temporarily storing tiles during traversal reordering.
///
/// Supports both in-memory and disk-backed storage, selected at runtime
/// via [`CacheType`].
pub enum TraversalCache<V: CacheValue> {
	/// In-memory cache using a concurrent hash map.
	Memory(DashMap<usize, Vec<V>>),
	/// Disk-backed cache storing values in binary files.
	///
	/// Each index gets its own subdirectory. Each `append`/`append_stream` call
	/// writes to a unique file within that subdirectory, so concurrent writers
	/// never touch the same file.
	Disk {
		path: PathBuf,
		next_writer_id: AtomicUsize,
		_marker: PhantomData<V>,
	},
}

impl<V: CacheValue> TraversalCache<V> {
	/// Create a new cache using the specified cache type.
	///
	/// * `InMemory` -> uses an in-process concurrent map.
	/// * `Disk(path)` -> creates a unique subdirectory under `path`.
	#[must_use]
	pub fn new(cache_type: &CacheType) -> Self {
		match cache_type {
			CacheType::InMemory => Self::Memory(DashMap::new()),
			CacheType::Disk(base_path) => {
				let path = base_path.join(format!("traversal_{}", Uuid::new_v4()));
				create_dir_all(&path).ok();
				Self::Disk {
					path,
					next_writer_id: AtomicUsize::new(0),
					_marker: PhantomData,
				}
			}
		}
	}

	/// Append values to the cache entry at `index`.
	///
	/// Creates a new entry if the index doesn't exist yet.
	#[context("Failed to append to traversal cache at index {}", index)]
	pub fn append(&self, index: usize, values: Vec<V>) -> Result<()> {
		match self {
			Self::Memory(map) => {
				map.entry(index).or_default().extend(values);
				Ok(())
			}
			Self::Disk {
				path, next_writer_id, ..
			} => {
				let writer_id = next_writer_id.fetch_add(1, Ordering::Relaxed);
				let dir_path = path.join(index.to_string());
				create_dir_all(&dir_path)?;
				let file_path = dir_path.join(format!("{writer_id:012}.bin"));
				let file = File::create(&file_path)?;
				let mut writer = BufWriter::new(file);
				for value in &values {
					value.write_to_cache(&mut writer)?;
				}
				writer.flush()?;
				Ok(())
			}
		}
	}

	/// Append values from a stream to the cache entry at `index`.
	///
	/// Each call writes to its own file, so concurrent callers appending to the
	/// same index will never interleave their data.
	#[context("Failed to append stream to traversal cache at index {}", index)]
	pub async fn append_stream<S>(&self, index: usize, stream: S) -> Result<()>
	where
		S: Stream<Item = V> + Send + Unpin,
	{
		match self {
			Self::Memory(map) => {
				let values: Vec<V> = stream.collect().await;
				map.entry(index).or_default().extend(values);
				Ok(())
			}
			Self::Disk {
				path, next_writer_id, ..
			} => {
				let writer_id = next_writer_id.fetch_add(1, Ordering::Relaxed);
				let dir_path = path.join(index.to_string());
				create_dir_all(&dir_path)?;
				let file_path = dir_path.join(format!("{writer_id:012}.bin"));
				let file = File::create(&file_path)?;
				let mut writer = BufWriter::new(file);
				futures::pin_mut!(stream);
				while let Some(value) = stream.next().await {
					value.write_to_cache(&mut writer)?;
				}
				writer.flush()?;
				Ok(())
			}
		}
	}

	/// Take and return all values at `index`, removing them from the cache.
	///
	/// Returns `Ok(None)` if no entry exists at the index.
	#[context("Failed to take from traversal cache at index {}", index)]
	pub fn take(&self, index: usize) -> Result<Option<Vec<V>>> {
		match self {
			Self::Memory(map) => Ok(map.remove(&index).map(|(_, v)| v)),
			Self::Disk { path, .. } => {
				let dir_path = path.join(index.to_string());
				if !dir_path.exists() {
					return Ok(None);
				}
				let data = Self::read_index_dir(&dir_path)?;
				remove_dir_all(&dir_path)?;
				if data.is_empty() {
					Ok(None)
				} else {
					Ok(Some(Self::buffer_to_values(&data)?))
				}
			}
		}
	}

	/// Take all values at `index` as a stream, removing them from the cache.
	///
	/// Returns `Ok(None)` if no entry exists at the index.
	#[context("Failed to take stream from traversal cache at index {}", index)]
	pub fn take_stream(&self, index: usize) -> Result<Option<BoxStream<'static, V>>>
	where
		V: Send + 'static,
	{
		match self {
			Self::Memory(map) => Ok(map.remove(&index).map(|(_, v)| futures::stream::iter(v).boxed())),
			Self::Disk { path, .. } => {
				let dir_path = path.join(index.to_string());
				if !dir_path.exists() {
					return Ok(None);
				}
				let data = Self::read_index_dir(&dir_path)?;
				remove_dir_all(&dir_path)?;
				if data.is_empty() {
					Ok(None)
				} else {
					let values = Self::buffer_to_values(&data)?;
					Ok(Some(futures::stream::iter(values).boxed()))
				}
			}
		}
	}

	/// Read all cache files for an index directory, sorted by filename to
	/// preserve insertion order.
	fn read_index_dir(dir_path: &Path) -> Result<Vec<u8>> {
		let mut entries: Vec<_> = read_dir(dir_path)?
			.filter_map(std::result::Result::ok)
			.filter(|e| e.path().extension().is_some_and(|ext| ext == "bin"))
			.collect();
		entries.sort_by_key(std::fs::DirEntry::file_name);
		let mut data = Vec::new();
		for entry in entries {
			File::open(entry.path())?.read_to_end(&mut data)?;
		}
		Ok(data)
	}

	/// Deserialize values from a binary buffer.
	fn buffer_to_values(buf: &[u8]) -> Result<Vec<V>> {
		let mut reader = Cursor::new(buf);
		let mut vec = Vec::new();
		while reader.position() < buf.len() as u64 {
			vec.push(V::read_from_cache(&mut reader)?);
		}
		Ok(vec)
	}

	/// Clean up cache resources.
	fn clean_up(&self) {
		match self {
			Self::Memory(map) => map.clear(),
			Self::Disk { path, .. } => {
				remove_dir_all(path).ok();
			}
		}
	}
}

impl<V: CacheValue> Drop for TraversalCache<V> {
	fn drop(&mut self) {
		self.clean_up();
	}
}

impl<V: CacheValue> Debug for TraversalCache<V> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Memory(map) => {
				write!(f, "TraversalCache::Memory({} entries)", map.len())
			}
			Self::Disk { path, .. } => {
				write!(f, "TraversalCache::Disk({})", path.display())
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;
	use tempfile::TempDir;

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	fn test_append_and_take(#[case] case: &str) -> Result<()> {
		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<String>::new(&cache_type);

		// Initially empty
		assert_eq!(cache.take(0)?, None);
		assert_eq!(cache.take(1)?, None);

		// Append to index 0
		cache.append(0, vec!["a".to_string(), "b".to_string()])?;

		// Append more to index 0
		cache.append(0, vec!["c".to_string()])?;

		// Append to different index
		cache.append(1, vec!["x".to_string()])?;

		// Take preserves order
		assert_eq!(
			cache.take(0)?,
			Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
		);

		// After take, index is empty
		assert_eq!(cache.take(0)?, None);

		// Other index still has data
		assert_eq!(cache.take(1)?, Some(vec!["x".to_string()]));

		Ok(())
	}

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	fn test_binary_values(#[case] case: &str) -> Result<()> {
		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<Vec<u8>>::new(&cache_type);

		cache.append(0, vec![vec![0, 1, 2], vec![255, 254]])?;
		cache.append(0, vec![vec![128]])?;

		assert_eq!(cache.take(0)?, Some(vec![vec![0, 1, 2], vec![255, 254], vec![128]]));

		Ok(())
	}

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	#[tokio::test]
	async fn test_append_stream(#[case] case: &str) -> Result<()> {
		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<String>::new(&cache_type);

		// Append via stream
		let stream = futures::stream::iter(vec!["a".to_string(), "b".to_string()]);
		cache.append_stream(0, stream).await?;

		// Append more via stream to same index
		let stream2 = futures::stream::iter(vec!["c".to_string()]);
		cache.append_stream(0, stream2).await?;

		// Take preserves order across stream appends
		assert_eq!(
			cache.take(0)?,
			Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
		);

		// After take, index is empty
		assert_eq!(cache.take(0)?, None);

		Ok(())
	}

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	#[tokio::test]
	async fn test_take_stream(#[case] case: &str) -> Result<()> {
		use futures::StreamExt;

		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<String>::new(&cache_type);

		// Append data
		cache.append(0, vec!["a".to_string(), "b".to_string()])?;
		cache.append(0, vec!["c".to_string()])?;

		// take_stream and collect
		let stream = cache.take_stream(0)?.unwrap();
		let collected: Vec<String> = stream.collect().await;
		assert_eq!(collected, vec!["a".to_string(), "b".to_string(), "c".to_string()]);

		// After take_stream, index is empty
		assert!(cache.take_stream(0)?.is_none());

		// Non-existent index returns None
		assert!(cache.take_stream(99)?.is_none());

		Ok(())
	}

	#[test]
	fn test_debug_format() {
		let mem_cache = TraversalCache::<String>::new(&CacheType::InMemory);
		assert!(format!("{mem_cache:?}").contains("Memory"));

		let tmp = TempDir::new().unwrap();
		let disk_cache = TraversalCache::<String>::new(&CacheType::Disk(tmp.path().to_path_buf()));
		assert!(format!("{disk_cache:?}").contains("Disk"));
	}
}
