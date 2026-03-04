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
	fs::{File, OpenOptions, create_dir_all, remove_dir_all, remove_file, write},
	io::{BufWriter, Cursor, Read, Write},
	marker::PhantomData,
	path::PathBuf,
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
	Disk { path: PathBuf, _marker: PhantomData<V> },
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
			Self::Disk { path, .. } => {
				let file_path = path.join(format!("{index}.bin"));
				let buffer = Self::values_to_buffer(&values)?;
				if file_path.exists() {
					OpenOptions::new().append(true).open(&file_path)?.write_all(&buffer)?;
				} else {
					write(&file_path, buffer)?;
				}
				Ok(())
			}
		}
	}

	/// Append values from a stream to the cache entry at `index`.
	///
	/// Unlike [`append`](Self::append), this consumes a stream directly, avoiding
	/// intermediate `Vec` allocation for the disk variant.
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
			Self::Disk { path, .. } => {
				let file_path = path.join(format!("{index}.bin"));
				let file = OpenOptions::new().create(true).append(true).open(&file_path)?;
				let mut writer = BufWriter::new(file);
				let mut buf = Vec::new();
				futures::pin_mut!(stream);
				while let Some(value) = stream.next().await {
					buf.clear();
					value.write_to_cache(&mut buf)?;
					writer.write_all(&buf)?;
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
				let file_path = path.join(format!("{index}.bin"));
				if file_path.exists() {
					let mut file = File::open(&file_path)?;
					let mut data = Vec::new();
					file.read_to_end(&mut data)?;
					remove_file(&file_path)?;
					Ok(Some(Self::buffer_to_values(&data)?))
				} else {
					Ok(None)
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
				let file_path = path.join(format!("{index}.bin"));
				if file_path.exists() {
					let mut file = File::open(&file_path)?;
					let mut data = Vec::new();
					file.read_to_end(&mut data)?;
					remove_file(&file_path)?;
					let len = data.len() as u64;
					let mut cursor = Cursor::new(data);
					let iter = std::iter::from_fn(move || {
						if cursor.position() >= len {
							None
						} else {
							Some(
								V::read_from_cache(&mut cursor)
									.expect("failed to deserialize from traversal cache"),
							)
						}
					});
					Ok(Some(futures::stream::iter(iter).boxed()))
				} else {
					Ok(None)
				}
			}
		}
	}

	/// Serialize values into a binary buffer.
	fn values_to_buffer(values: &[V]) -> Result<Vec<u8>> {
		let mut buf = Vec::new();
		for value in values {
			value.write_to_cache(&mut buf)?;
		}
		Ok(buf)
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
