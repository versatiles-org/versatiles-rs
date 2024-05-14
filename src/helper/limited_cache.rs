use std::{collections::HashMap, fmt::Debug, hash::Hash, mem::size_of, ops::Div};

pub struct LimitedCache<K, V> {
	cache: HashMap<K, (V, u64)>,
	max_length: usize,
	last_index: u64,
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
			last_index: 0,
		}
	}
	pub fn get(&mut self, key: &K) -> Option<V> {
		if let Some(value) = self.cache.get_mut(key) {
			self.last_index += 1;
			value.1 = self.last_index;
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
		let mut latest_access: Vec<u64> = self.cache.values().map(|e| e.1).collect();
		latest_access.sort_unstable();
		let median = latest_access[latest_access.len().div(2)];
		self.cache.retain(|_, e| {
			if e.1 <= median {
				false
			} else {
				e.1 = 0;
				true
			}
		});
	}
}

impl<K, V> Debug for LimitedCache<K, V> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LimitedCache")
			.field("length", &self.cache.len())
			.field("max_length", &self.max_length)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::LimitedCache;

	#[test]
	fn test_cache_initialization() {
		let cache: LimitedCache<u64, i32> = LimitedCache::with_maximum_size(100);
		assert_eq!(
			cache.max_length,
			100 / (std::mem::size_of::<u64>() + std::mem::size_of::<i32>())
		);
	}

	#[test]
	fn test_add_and_get_items() {
		let mut cache = LimitedCache::with_maximum_size(10 * (std::mem::size_of::<i32>() + std::mem::size_of::<i32>()));
		cache.add(1, 100);
		cache.add(2, 200);

		assert_eq!(cache.get(&1), Some(100));
		assert_eq!(cache.get(&2), Some(200));
		assert_eq!(cache.get(&3), None); // Key 3 was never added
	}

	#[test]
	fn test_capacity_and_cleanup() {
		let test = |max: u64, result: &[u64]| {
			let mut cache: LimitedCache<u64, u64> = LimitedCache::with_maximum_size(10 * (std::mem::size_of::<u64>()));
			for i in 0..=max {
				cache.add(i, i * 100);
				cache.get(&i);
			}
			let mut list: Vec<u64> = Vec::new();
			for i in 0..=9 {
				list.push(if cache.get(&i).is_some() { 1 } else { 0 });
			}
			assert_eq!(list.as_slice(), result, "error for test index {max}");
		};

		test(0, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
		test(1, &[1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
		test(2, &[1, 1, 1, 0, 0, 0, 0, 0, 0, 0]);
		test(3, &[1, 1, 1, 1, 0, 0, 0, 0, 0, 0]);
		test(4, &[1, 1, 1, 1, 1, 0, 0, 0, 0, 0]);
		test(5, &[0, 0, 0, 1, 1, 1, 0, 0, 0, 0]);
		test(6, &[0, 0, 0, 1, 1, 1, 1, 0, 0, 0]);
		test(7, &[0, 0, 0, 1, 1, 1, 1, 1, 0, 0]);
		test(8, &[0, 0, 0, 0, 0, 0, 1, 1, 1, 0]);
		test(9, &[0, 0, 0, 0, 0, 0, 1, 1, 1, 1]);
	}
}
