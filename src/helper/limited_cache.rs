use std::{collections::HashMap, hash::Hash, mem::size_of, ops::Div};

pub struct LimitedCache<K, V> {
	cache: HashMap<K, (V, usize)>,
	max_length: usize,
}

impl<K, V> LimitedCache<K, V>
where
	V: Clone,
	K: Clone + Eq + Hash + PartialEq,
{
	pub fn with_maximum_size(maximum_size: usize) -> Self {
		Self {
			cache: HashMap::new(),
			max_length: maximum_size.div(size_of::<K>() + size_of::<V>()),
		}
	}
	pub fn get(&mut self, key: &K) -> Option<V> {
		if let Some(value) = self.cache.get_mut(key) {
			value.1 += 1;
			Some(value.0.clone())
		} else {
			None
		}
	}
	pub fn add(&mut self, key: K, value: V) -> V {
		if self.cache.len() >= self.max_length {
			self.cleanup();
		}

		self.cache.entry(key).or_insert((value, 0)).0.clone()
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
