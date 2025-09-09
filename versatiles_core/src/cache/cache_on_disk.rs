use super::traits::{Cache, CacheKey, CacheValue};
use anyhow::Result;
use std::{
	fs::{File, OpenOptions, create_dir_all, remove_dir_all, remove_file, write},
	io::{Read, Write},
	marker::PhantomData,
	path::{Path, PathBuf},
};

pub struct OnDiskCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	path: PathBuf, // path to cache directory
	_marker_k: PhantomData<K>,
	_marker_v: PhantomData<V>,
}

#[allow(clippy::new_without_default)]
impl<K, V> OnDiskCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	pub fn new(path: PathBuf) -> Self {
		create_dir_all(&path).ok();
		Self {
			path,
			_marker_k: PhantomData,
			_marker_v: PhantomData,
		}
	}

	fn get_entry_path(&self, key: &K) -> PathBuf {
		let mut p = self.path.clone();
		p.push(format!("{}.tmp", key.to_cache_key()));
		p
	}

	fn buffer_to_value(buf: &[u8]) -> Vec<V> {
		let mut result = Vec::new();
		let mut pos = 0;
		while pos < buf.len() {
			let length = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
			pos += 4;
			let value = V::from_cache_buffer(&buf[pos..pos + length]);
			pos += length;
			result.push(value);
		}
		result
	}

	fn values_to_buffer(values: Vec<V>) -> Vec<u8> {
		let mut buf = Vec::new();
		for value in values {
			let value_buf = value.to_cache_buffer();
			let length = value_buf.len() as u32;
			buf.extend(&length.to_le_bytes());
			buf.extend(value_buf);
		}
		buf
	}

	fn read_file(&self, entry_path: &Path) -> Result<Option<Vec<V>>> {
		if entry_path.exists() {
			let mut file = File::open(entry_path)?;
			let mut data = Vec::new();
			file.read_to_end(&mut data)?;
			Ok(Some(Self::buffer_to_value(&data)))
		} else {
			Ok(None)
		}
	}
}

impl<K, V> Cache<K, V> for OnDiskCache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
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
		create_dir_all(entry_path.parent().unwrap())?;
		write(entry_path, Self::values_to_buffer(values))?;
		Ok(())
	}

	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()> {
		let entry_path = self.get_entry_path(key);
		let buffer = Self::values_to_buffer(values);
		if entry_path.exists() {
			OpenOptions::new().append(true).open(entry_path)?.write_all(&buffer)?;
		} else {
			create_dir_all(entry_path.parent().unwrap())?;
			write(entry_path, buffer)?;
		}
		Ok(())
	}

	fn clean_up(&mut self) {
		remove_dir_all(&self.path).ok();
	}
}
