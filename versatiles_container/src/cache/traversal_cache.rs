//! Specialized cache for tile traversal reordering.
//!
//! [`TraversalCache<V>`] temporarily stores tiles during the Push phase of a
//! traversal and retrieves them during the Pop phase, allowing tiles to be
//! reordered across zoom-level boundaries.
//!
//! Two storage backends are available, selected at runtime via [`CacheType`]:
//!
//! - **`InMemory`** — concurrent [`DashMap`] backed by RAM. Fast, but memory
//!   usage grows with the number of cached tiles.
//! - **`Disk`** — each `append`/`append_stream` call writes to its own
//!   uniquely-named file, so concurrent writers never interleave data. An
//!   in-memory index tracks which files belong to each cache entry.
//!
//! # Thread safety
//!
//! All public methods are safe to call concurrently from multiple tasks.
//! Concurrent writes to the same index are isolated via per-call files (Disk)
//! or [`DashMap`] sharding (Memory).

use crate::cache::{cache_type::CacheType, traits::CacheValue};
use anyhow::Result;
use dashmap::DashMap;
use futures::{Stream, StreamExt, stream::BoxStream};
use std::{
	fmt::Debug,
	fs::{File, create_dir_all, remove_dir_all},
	io::{BufReader, BufWriter, Read, Write},
	marker::PhantomData,
	path::{Path, PathBuf},
	sync::atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
use versatiles_derive::context;

/// A thin [`Read`] wrapper that tracks the number of bytes consumed.
struct CountingReader<R> {
	inner: R,
	position: u64,
}

impl<R: Read> Read for CountingReader<R> {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		let n = self.inner.read(buf)?;
		self.position += n as u64;
		Ok(n)
	}
}

/// A cache for temporarily storing values during traversal reordering.
///
/// Supports both in-memory and disk-backed storage, selected at runtime
/// via [`CacheType`].
///
/// # Disk layout
///
/// ```text
/// <base_path>/traversal_<uuid>/
///   <index>/
///     000000000000.bin   ← one file per append/append_stream call
///     000000000001.bin
///     ...
/// ```
///
/// Files are cleaned up when values are taken or when the cache is dropped.
pub enum TraversalCache<V: CacheValue> {
	/// In-memory cache using a concurrent hash map.
	Memory(DashMap<usize, Vec<V>>),
	/// Disk-backed cache storing values in binary files.
	Disk {
		/// Root directory for this cache instance.
		path: PathBuf,
		/// Atomic counter for generating unique file names.
		next_writer_id: AtomicUsize,
		/// Tracks which files belong to each cache index.
		file_index: DashMap<usize, Vec<PathBuf>>,
		_marker: PhantomData<V>,
	},
}

impl<V: CacheValue> TraversalCache<V> {
	/// Create a new cache backed by the given [`CacheType`].
	///
	/// For [`CacheType::Disk`], a unique subdirectory is created immediately.
	///
	/// # Errors
	///
	/// Returns an error if the cache directory cannot be created (Disk mode).
	pub fn new(cache_type: &CacheType) -> Result<Self> {
		Ok(match cache_type {
			CacheType::InMemory => Self::Memory(DashMap::new()),
			CacheType::Disk(base_path) => {
				let path = base_path.join(format!("traversal_{}", Uuid::new_v4()));
				create_dir_all(&path)?;
				Self::Disk {
					path,
					next_writer_id: AtomicUsize::new(0),
					file_index: DashMap::new(),
					_marker: PhantomData,
				}
			}
		})
	}

	/// Append values from a stream to the cache entry at `index`.
	///
	/// Values are consumed one at a time, so peak memory usage is independent
	/// of the stream length. In Disk mode, each call writes to its own file,
	/// so concurrent callers appending to the same index never interleave data.
	#[context("Failed to append stream to traversal cache at index {}", index)]
	pub async fn append_stream<S>(&self, index: usize, mut stream: S) -> Result<()>
	where
		S: Stream<Item = V> + Send + Unpin,
	{
		match self {
			Self::Memory(map) => {
				while let Some(value) = stream.next().await {
					map.entry(index).or_default().push(value);
				}
				Ok(())
			}
			Self::Disk {
				path,
				next_writer_id,
				file_index,
				..
			} => {
				let (mut writer, file_path) = Self::create_cache_file(path, index, next_writer_id)?;
				while let Some(value) = stream.next().await {
					value.write_to_cache(&mut writer)?;
				}
				writer.flush()?;
				file_index.entry(index).or_default().push(file_path);
				Ok(())
			}
		}
	}

	/// Take all values at `index` as a stream, removing them from the cache.
	///
	/// In Disk mode, files are read concurrently on blocking threads and
	/// values are streamed through a bounded channel (capacity 64), so only
	/// a small number of values are in memory at a time regardless of total
	/// data size.
	///
	/// **Ordering:** In Disk mode, values from different files (i.e. different
	/// `append`/`append_stream` calls) may arrive in any order. Values within
	/// a single file preserve their original order.
	///
	/// Returns `Ok(None)` if no entry exists at the index.
	///
	/// # Panics
	///
	/// Panics if called outside a Tokio runtime (Disk mode only).
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
				let (tx, rx) = tokio::sync::mpsc::channel::<V>(64);
				for file_path in files {
					let tx = tx.clone();
					tokio::task::spawn_blocking(move || match Self::iter_values_from_file(&file_path) {
						Err(e) => log::warn!("failed to open cache file {}: {e}", file_path.display()),
						Ok(iter) => {
							for result in iter {
								match result {
									Ok(value) => {
										if tx.blocking_send(value).is_err() {
											break;
										}
									}
									Err(e) => {
										log::warn!(
											"failed to deserialize value from cache file {}: {e}",
											file_path.display()
										);
										break;
									}
								}
							}
						}
					});
				}
				drop(tx);
				let stream = futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (v, rx)) });
				let cleanup = futures::stream::once(async move {
					let _ = remove_dir_all(dir_path);
				})
				.filter_map(|()| futures::future::ready(None));
				Ok(Some(stream.chain(cleanup).boxed()))
			}
		}
	}

	/// Create a uniquely-named cache file for writing at `index`.
	///
	/// Returns a buffered writer and the file path for later registration
	/// in `file_index`.
	fn create_cache_file(path: &Path, index: usize, next_writer_id: &AtomicUsize) -> Result<(BufWriter<File>, PathBuf)> {
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
	fn take_index_files(file_index: &DashMap<usize, Vec<PathBuf>>, index: usize) -> Option<Vec<PathBuf>> {
		match file_index.remove(&index) {
			Some((_, files)) if !files.is_empty() => Some(files),
			_ => None,
		}
	}

	/// Open a cache file and return an iterator that deserializes values
	/// one at a time using buffered I/O.
	fn iter_values_from_file(path: &Path) -> Result<impl Iterator<Item = Result<V>> + use<V>> {
		let file = File::open(path)?;
		let file_len = file.metadata()?.len();
		let mut reader = CountingReader {
			inner: BufReader::new(file),
			position: 0,
		};
		Ok(std::iter::from_fn(move || {
			if reader.position >= file_len {
				return None;
			}
			Some(V::read_from_cache(&mut reader))
		}))
	}

	/// Clean up all cache resources (files and directories).
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
	#[tokio::test]
	async fn test_append_and_take_stream(#[case] case: &str) -> Result<()> {
		use futures::StreamExt;

		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<String>::new(&cache_type)?;

		// Initially empty
		assert!(cache.take_stream(0)?.is_none());
		assert!(cache.take_stream(1)?.is_none());

		// Append via stream to index 0
		cache
			.append_stream(0, futures::stream::iter(vec!["a".to_string(), "b".to_string()]))
			.await?;

		// Append more via stream to same index
		cache
			.append_stream(0, futures::stream::iter(vec!["c".to_string()]))
			.await?;

		// Append to different index
		cache
			.append_stream(1, futures::stream::iter(vec!["x".to_string()]))
			.await?;

		// take_stream and collect (order across files is non-deterministic in Disk mode)
		let stream = cache.take_stream(0)?.unwrap();
		let mut collected: Vec<String> = stream.collect().await;
		collected.sort();
		assert_eq!(collected, vec!["a".to_string(), "b".to_string(), "c".to_string()]);

		// After take_stream, index is empty
		assert!(cache.take_stream(0)?.is_none());

		// Other index still has data
		let stream1 = cache.take_stream(1)?.unwrap();
		let collected1: Vec<String> = stream1.collect().await;
		assert_eq!(collected1, vec!["x".to_string()]);

		// Non-existent index returns None
		assert!(cache.take_stream(99)?.is_none());

		Ok(())
	}

	#[rstest]
	#[case::mem("mem")]
	#[case::disk("disk")]
	#[tokio::test]
	async fn test_binary_values_stream(#[case] case: &str) -> Result<()> {
		use futures::StreamExt;

		let cache_type = match case {
			"mem" => CacheType::InMemory,
			"disk" => CacheType::Disk(TempDir::new()?.path().to_path_buf()),
			_ => panic!("unknown case"),
		};
		let cache = TraversalCache::<Vec<u8>>::new(&cache_type)?;

		cache
			.append_stream(0, futures::stream::iter(vec![vec![0, 1, 2], vec![255, 254]]))
			.await?;
		cache.append_stream(0, futures::stream::iter(vec![vec![128]])).await?;

		let stream = cache.take_stream(0)?.unwrap();
		let mut collected: Vec<Vec<u8>> = stream.collect().await;
		collected.sort();
		assert_eq!(collected, vec![vec![0, 1, 2], vec![128], vec![255, 254]]);

		Ok(())
	}

	#[test]
	fn test_debug_format() {
		let mem_cache = TraversalCache::<String>::new(&CacheType::InMemory).unwrap();
		assert!(format!("{mem_cache:?}").contains("Memory"));

		let tmp = TempDir::new().unwrap();
		let disk_cache = TraversalCache::<String>::new(&CacheType::Disk(tmp.path().to_path_buf())).unwrap();
		assert!(format!("{disk_cache:?}").contains("Disk"));
	}
}
