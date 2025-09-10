use super::traits::{Cache, CacheKey, CacheValue};
use anyhow::Result;
use std::{
	fmt::Debug,
	fs::{File, OpenOptions, create_dir_all, remove_dir_all, remove_file, write},
	io::{Cursor, Read, Write},
	marker::PhantomData,
	path::{Path, PathBuf},
};

pub struct OnDiskCache<K: CacheKey, V: CacheValue> {
	path: PathBuf, // path to cache directory
	_marker_k: PhantomData<K>,
	_marker_v: PhantomData<V>,
}

#[allow(clippy::new_without_default)]
impl<K: CacheKey, V: CacheValue> OnDiskCache<K, V> {
	pub fn new(path: PathBuf) -> Self {
		create_dir_all(&path).ok();
		Self {
			path,
			_marker_k: PhantomData,
			_marker_v: PhantomData,
		}
	}

	fn get_entry_path(&self, key: &K) -> PathBuf {
		// ensure the name is a valid file name by replacing all non unix path characters with '%' followed by the hexadecimal
		let name = key
			.to_cache_key()
			.bytes()
			.map(|b| {
				if (b as char).is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b',' {
					(b as char).to_string()
				} else {
					format!("%{:02x}", b)
				}
			})
			.collect::<String>();
		let mut p = self.path.clone();
		p.push(format!("{}.tmp", name));
		p
	}

	fn buffer_to_values(buf: &[u8]) -> Result<Vec<V>> {
		let mut reader = Cursor::new(buf);
		let mut vec = Vec::new();
		while reader.position() < buf.len() as u64 {
			let value = V::read_from_cache(&mut reader)?;
			vec.push(value);
		}
		Ok(vec)
	}

	fn values_to_buffer(values: Vec<V>) -> Result<Vec<u8>> {
		let mut buf = Vec::new();
		for value in values {
			value.write_to_cache(&mut buf)?;
		}
		Ok(buf)
	}

	fn read_file(&self, entry_path: &Path) -> Result<Option<Vec<V>>> {
		if entry_path.exists() {
			let mut file = File::open(entry_path)?;
			let mut data = Vec::new();
			file.read_to_end(&mut data)?;
			Ok(Some(Self::buffer_to_values(&data)?))
		} else {
			Ok(None)
		}
	}
}

impl<K: CacheKey, V: CacheValue> Cache<K, V> for OnDiskCache<K, V> {
	fn contains_key(&self, key: &K) -> bool {
		self.get_entry_path(key).exists()
	}

	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>> {
		self.read_file(&self.get_entry_path(key))
	}

	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>> {
		let entry_path = self.get_entry_path(key);
		let values = self.read_file(&entry_path)?;
		if entry_path.exists() {
			remove_file(&entry_path)?;
		}
		Ok(values)
	}

	fn insert(&mut self, key: &K, values: Vec<V>) -> Result<()> {
		let entry_path = self.get_entry_path(key);
		write(entry_path, Self::values_to_buffer(values)?)?;
		Ok(())
	}

	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()> {
		let entry_path = self.get_entry_path(key);
		let buffer = Self::values_to_buffer(values)?;
		if entry_path.exists() {
			OpenOptions::new().append(true).open(entry_path)?.write_all(&buffer)?;
		} else {
			write(entry_path, buffer)?;
		}
		Ok(())
	}

	fn clean_up(&mut self) {
		remove_dir_all(&self.path).ok();
	}
}

impl<K: CacheKey, V: CacheValue> Debug for OnDiskCache<K, V> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("OnDiskCache").field("path", &self.path).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tempfile::TempDir;

	fn new_cache() -> (TempDir, OnDiskCache<String, String>) {
		let dir = tempfile::tempdir().expect("tempdir");
		let cache_path = dir.path().join("cache");
		let cache = OnDiskCache::<String, String>::new(cache_path);
		(dir, cache)
	}

	fn v(s: &[&str]) -> Vec<String> {
		s.iter().map(|b| b.to_string()).collect()
	}

	#[test]
	fn get_entry_path_encodes_non_alnum() {
		let (_tmp, cache) = new_cache();
		// simple alnum stays unchanged
		let p1 = cache.get_entry_path(&"abc-_.,".to_string());
		assert_eq!(p1.file_name().unwrap().to_str().unwrap(), "abc-_.,.tmp");
		// slash and space are encoded
		let p2 = cache.get_entry_path(&"a/b c".to_string());
		assert_eq!(p2.file_name().unwrap().to_str().unwrap(), "a%2fb%20c.tmp");
		// unicode bytes are percent-encoded (ä = 0xC3 0xA4)
		let p3 = cache.get_entry_path(&"ä".to_string());
		assert_eq!(p3.file_name().unwrap().to_str().unwrap(), "%c3%a4.tmp");
	}

	#[test]
	fn insert_get_append_remove_flow_strings() {
		let (tmp, mut cache) = new_cache();
		let k = "key:1".to_string();
		// initially
		assert!(!cache.contains_key(&k));
		assert!(cache.get_clone(&k).unwrap().is_none());

		// insert
		cache.insert(&k, v(&["a", "b"])).unwrap();
		assert!(cache.contains_key(&k));
		assert_eq!(cache.get_clone(&k).unwrap(), Some(v(&["a", "b"])));
		// file exists on disk
		let entry = cache.get_entry_path(&k);
		assert!(entry.exists());

		// append keeps order
		cache.append(&k, v(&["c", "d"])).unwrap();
		assert_eq!(cache.get_clone(&k).unwrap(), Some(v(&["a", "b", "c", "d"])));

		// remove returns previous values and deletes file
		let prev = cache.remove(&k).unwrap();
		assert_eq!(prev, Some(v(&["a", "b", "c", "d"])));
		assert!(!cache.contains_key(&k));
		assert!(!entry.exists());

		// clean_up removes the whole cache directory
		let cache_dir = cache.path.clone();
		cache.clean_up();
		assert!(!cache_dir.exists());
		// tempdir itself still exists
		assert!(tmp.path().exists());
	}

	#[test]
	fn binary_values_roundtrip_and_append() {
		let dir = tempfile::tempdir().expect("tempdir");
		let cache_path = dir.path().join("cache");
		let mut cache = OnDiskCache::<String, Vec<u8>>::new(cache_path);
		let k = "blob".to_string();

		// write binary chunks (including non-UTF8)
		cache.insert(&k, vec![vec![0, 255], vec![1, 2, 3]]).unwrap();
		assert_eq!(cache.get_clone(&k).unwrap(), Some(vec![vec![0, 255], vec![1, 2, 3]]));
		cache.append(&k, vec![vec![9, 9]]).unwrap();
		assert_eq!(
			cache.get_clone(&k).unwrap(),
			Some(vec![vec![0, 255], vec![1, 2, 3], vec![9, 9]])
		);
	}

	#[test]
	fn append_creates_file_if_missing() {
		let (_tmp, mut cache) = new_cache();
		let k = "new-key".to_string();
		assert!(!cache.contains_key(&k));
		cache.append(&k, v(&["v1"])).unwrap();
		assert!(cache.contains_key(&k));
		assert_eq!(cache.get_clone(&k).unwrap(), Some(v(&["v1"])));
	}
}
