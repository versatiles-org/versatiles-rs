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
	fs::{File, create_dir_all, remove_dir_all},
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
	/// Each `append`/`append_stream` call writes to a unique file, so concurrent
	/// writers never touch the same file. An in-memory index tracks which files
	/// belong to each cache entry, so stale files on disk are ignored.
	Disk {
		path: PathBuf,
		next_writer_id: AtomicUsize,
		file_index: DashMap<usize, Vec<PathBuf>>,
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
					file_index: DashMap::new(),
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
				path,
				next_writer_id,
				file_index,
				..
			} => {
				let (mut writer, file_path) = Self::create_cache_file(path, index, next_writer_id)?;
				for value in &values {
					value.write_to_cache(&mut writer)?;
				}
				writer.flush()?;
				file_index.entry(index).or_default().push(file_path);
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
				path,
				next_writer_id,
				file_index,
				..
			} => {
				let (mut writer, file_path) = Self::create_cache_file(path, index, next_writer_id)?;
				futures::pin_mut!(stream);
				while let Some(value) = stream.next().await {
					value.write_to_cache(&mut writer)?;
				}
				writer.flush()?;
				file_index.entry(index).or_default().push(file_path);
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
			Self::Disk { path, file_index, .. } => {
				let Some(files) = Self::take_index_files(file_index, index) else {
					return Ok(None);
				};
				// Read and deserialize one file at a time to avoid loading all
				// raw bytes into memory simultaneously.
				let mut values = Vec::new();
				for file_path in &files {
					let mut data = Vec::new();
					File::open(file_path)?.read_to_end(&mut data)?;
					values.extend(Self::buffer_to_values(&data)?);
				}
				remove_dir_all(path.join(index.to_string())).ok();
				Ok(Some(values))
			}
		}
	}

	/// Take all values at `index` as a stream, removing them from the cache.
	///
	/// Files are read lazily one at a time, so only one file's worth of data
	/// is in memory at any point.
	///
	/// Returns `Ok(None)` if no entry exists at the index.
	#[context("Failed to take stream from traversal cache at index {}", index)]
	pub fn take_stream(&self, index: usize) -> Result<Option<BoxStream<'static, V>>>
	where
		V: Send + 'static,
	{
		match self {
			Self::Memory(map) => Ok(map.remove(&index).map(|(_, v)| futures::stream::iter(v).boxed())),
			Self::Disk { path, file_index, .. } => {
				let Some(files) = Self::take_index_files(file_index, index) else {
					return Ok(None);
				};
				let dir_path = path.join(index.to_string());
				let mut file_iter = files.into_iter();
				let mut current_values: std::vec::IntoIter<V> = Vec::new().into_iter();
				let iter = std::iter::from_fn(move || {
					loop {
						if let Some(v) = current_values.next() {
							return Some(v);
						}
						let file_path = file_iter.next()?;
						let mut data = Vec::new();
						File::open(&file_path).ok()?.read_to_end(&mut data).ok()?;
						std::fs::remove_file(&file_path).ok();
						current_values = Self::buffer_to_values(&data).ok()?.into_iter();
					}
				});
				// Append a cleanup action that removes the now-empty directory
				// after the last value has been yielded.
				let cleanup = std::iter::once_with(move || {
					remove_dir_all(&dir_path).ok();
					None
				})
				.flatten();
				Ok(Some(futures::stream::iter(iter.chain(cleanup)).boxed()))
			}
		}
	}

	/// Create a new uniquely-named cache file for writing at `index`.
	///
	/// Returns a buffered writer and the file path (for later registration
	/// in `file_index`).
	fn create_cache_file(
		path: &Path,
		index: usize,
		next_writer_id: &AtomicUsize,
	) -> Result<(BufWriter<File>, PathBuf)> {
		let writer_id = next_writer_id.fetch_add(1, Ordering::Relaxed);
		let dir_path = path.join(index.to_string());
		create_dir_all(&dir_path)?;
		let file_path = dir_path.join(format!("{writer_id:012}.bin"));
		let file = File::create(&file_path)?;
		Ok((BufWriter::new(file), file_path))
	}

	/// Remove and return the tracked file list for the given index.
	///
	/// Returns `None` if no files were registered for this index.
	fn take_index_files(
		file_index: &DashMap<usize, Vec<PathBuf>>,
		index: usize,
	) -> Option<Vec<PathBuf>> {
		match file_index.remove(&index) {
			Some((_, files)) if !files.is_empty() => Some(files),
			_ => None,
		}
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
