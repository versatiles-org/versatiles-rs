use super::{CacheKey, CacheValue};
use anyhow::Result;
use std::fmt::Debug;

pub trait Cache<K: CacheKey, V: CacheValue>: Debug {
	fn contains_key(&self, key: &K) -> bool;
	fn get_clone(&self, key: &K) -> Result<Option<Vec<V>>>;
	fn remove(&mut self, key: &K) -> Result<Option<Vec<V>>>;
	fn insert(&mut self, key: &K, values: Vec<V>) -> Result<()>;
	fn append(&mut self, key: &K, values: Vec<V>) -> Result<()>;
	fn clean_up(&mut self);
}
