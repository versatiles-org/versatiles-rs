use super::{CacheKey, CacheValue};
use anyhow::Result;

pub trait Cache<K, V>
where
	K: CacheKey,
	V: CacheValue,
{
	fn contains_key(&self, key: &K) -> bool;
	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>>;
	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>>;
	fn insert(&mut self, key: &K, values: Vec<V>) -> Result<()>;
	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()>;
	fn clean_up(&mut self);
}
