use std::{collections::HashMap, hash::Hash, mem::size_of, ops::Div};

pub struct LimitedCache<K, V> {
	cache: HashMap<K, (V, usize)>,
	max_length: usize,
}

impl<K, V> LimitedCache<K, V>
where
	K: Clone + Eq + Hash + PartialEq,
{
	pub fn with_maximum_size(maximum_size: usize) -> Self {
		Self {
			cache: HashMap::new(),
			max_length: maximum_size.div(size_of::<K>() + size_of::<V>()),
		}
	}
	pub fn cache<'a, F>(&'a mut self, key: K, mut callback: F) -> &'a V
	where
		F: FnMut() -> V,
	{
		if self.cache.len() >= self.max_length {
			self.cleanup();
		}

		let entry = self.cache.entry(key).or_insert_with(|| (callback(), 0));
		entry.1 += 1;
		&entry.0
	}
	#[allow(dead_code)]
	fn get(&mut self, key: &K) -> Option<&V> {
		if let Some(entry) = self.cache.get_mut(key) {
			entry.1 += 1;
			Some(&entry.0)
		} else {
			None
		}
	}
	#[allow(dead_code)]
	fn insert(&mut self, key: K, value: V) -> Option<V> {
		if self.cache.len() >= self.max_length {
			self.cleanup();
		}

		self.cache.insert(key, (value, 0)).map(|e| e.0)
	}
	fn cleanup(&mut self) {
		let mut sizes: Vec<usize> = self.cache.values().map(|e| e.1).collect();
		sizes.sort_unstable();
		let median = sizes[sizes.len().div(2)];
		self.cache.retain(|_, e| {
			if e.1 < median {
				false
			} else {
				e.1 = 0;
				true
			}
		});
	}
}
