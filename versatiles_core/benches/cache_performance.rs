use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dashmap::DashMap;
use moka::future::Cache;
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;
use versatiles_core::TileCoord;

// Type aliases for complex types
type PyramidData = Vec<(TileCoord, Option<Arc<TileData>>)>;
type PyramidCache = Arc<DashMap<TileCoord, PyramidData>>;
type PyramidMutexCache = Arc<AsyncMutex<HashMap<TileCoord, PyramidData>>>;

// Simulated tile data (simplified version of DynamicImage)
#[derive(Clone, Debug)]
struct TileData {
	width: u32,
	height: u32,
	_data: Vec<u8>,
}

impl TileData {
	fn new(width: u32, height: u32) -> Self {
		Self {
			width,
			height,
			_data: vec![0u8; (width * height * 4) as usize], // RGBA
		}
	}

	fn size_bytes(&self) -> u32 {
		self.width * self.height * 4
	}
}

// Old approach: Mutex-based LRU cache (for comparison)
struct MutexCache {
	cache: Arc<AsyncMutex<HashMap<TileCoord, Arc<TileData>>>>,
	max_entries: usize,
}

impl MutexCache {
	fn new(max_entries: usize) -> Self {
		Self {
			cache: Arc::new(AsyncMutex::new(HashMap::new())),
			max_entries,
		}
	}

	async fn get(&self, key: &TileCoord) -> Option<Arc<TileData>> {
		let cache = self.cache.lock().await;
		cache.get(key).cloned()
	}

	async fn insert(&self, key: TileCoord, value: Arc<TileData>) {
		let mut cache = self.cache.lock().await;
		if cache.len() >= self.max_entries {
			// Simple eviction: remove first entry (not LRU, but simpler for benchmark)
			if let Some(k) = cache.keys().next().copied() {
				cache.remove(&k);
			}
		}
		cache.insert(key, value);
	}
}

// Benchmark: Moka cache vs Mutex cache - sequential access
fn bench_cache_sequential(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("cache_sequential");

	// Test data: 1000 unique tiles
	let coords: Vec<TileCoord> = (0..10)
		.flat_map(|z| {
			let max = 2u32.pow(u32::from(z));
			(0..max.min(10)).flat_map(move |x| (0..max.min(10)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(1000)
		.collect();

	let tile_data = Arc::new(TileData::new(256, 256));

	group.throughput(Throughput::Elements(coords.len() as u64));

	// Moka cache (lock-free)
	group.bench_function("moka_insert_sequential", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache: Cache<TileCoord, Arc<TileData>> = Cache::builder()
					.max_capacity(512 * 1024 * 1024) // 512MB
					.weigher(|_k, v: &Arc<TileData>| v.size_bytes())
					.build();

				for coord in &coords {
					cache.insert(*coord, Arc::clone(&tile_data)).await;
				}

				black_box(cache.entry_count())
			})
		});
	});

	// Mutex cache (locked)
	group.bench_function("mutex_insert_sequential", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache = MutexCache::new(1000);

				for coord in &coords {
					cache.insert(*coord, Arc::clone(&tile_data)).await;
				}

				black_box(cache.cache.lock().await.len())
			})
		});
	});

	group.finish();
}

// Benchmark: Concurrent cache access with high contention
fn bench_cache_concurrent_writes(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("cache_concurrent_writes");

	let coords: Vec<TileCoord> = (0..10)
		.flat_map(|z| {
			let max = 2u32.pow(u32::from(z));
			(0..max.min(10)).flat_map(move |x| (0..max.min(10)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(100)
		.collect();

	let tile_data = Arc::new(TileData::new(256, 256));
	let total_ops = (coords.len() * 10) as u64;

	group.throughput(Throughput::Elements(total_ops));

	// Moka cache - concurrent writes
	group.bench_function("moka_concurrent_writes", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache: Arc<Cache<TileCoord, Arc<TileData>>> = Arc::new(
					Cache::builder()
						.max_capacity(512 * 1024 * 1024)
						.weigher(|_k, v: &Arc<TileData>| v.size_bytes())
						.build(),
				);

				let handles: Vec<_> = (0..10)
					.map(|_| {
						let cache = Arc::clone(&cache);
						let coords = coords.clone();
						let tile_data = Arc::clone(&tile_data);
						tokio::spawn(async move {
							for coord in coords {
								cache.insert(coord, Arc::clone(&tile_data)).await;
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}

				black_box(cache.entry_count())
			})
		});
	});

	// Mutex cache - concurrent writes
	group.bench_function("mutex_concurrent_writes", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache = Arc::new(MutexCache::new(100));

				let handles: Vec<_> = (0..10)
					.map(|_| {
						let cache = Arc::clone(&cache);
						let coords = coords.clone();
						let tile_data = Arc::clone(&tile_data);
						tokio::spawn(async move {
							for coord in coords {
								cache.insert(coord, Arc::clone(&tile_data)).await;
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}

				black_box(cache.cache.lock().await.len())
			})
		});
	});

	group.finish();
}

// Benchmark: Cache hit ratio performance
fn bench_cache_hit_performance(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("cache_hit_performance");

	// Create a smaller set of coords that will fit in cache
	let coords: Vec<TileCoord> = (0..5)
		.flat_map(|z| {
			let max = 2u32.pow(u32::from(z));
			(0..max.min(5)).flat_map(move |x| (0..max.min(5)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(50)
		.collect();

	let tile_data = Arc::new(TileData::new(256, 256));

	// Pre-populate caches
	let moka_cache: Cache<TileCoord, Arc<TileData>> = rt.block_on(async {
		let cache = Cache::builder()
			.max_capacity(512 * 1024 * 1024)
			.weigher(|_k, v: &Arc<TileData>| v.size_bytes())
			.build();

		for coord in &coords {
			cache.insert(*coord, Arc::clone(&tile_data)).await;
		}

		cache
	});

	let mutex_cache = rt.block_on(async {
		let cache = MutexCache::new(100);
		for coord in &coords {
			cache.insert(*coord, Arc::clone(&tile_data)).await;
		}
		cache
	});

	let total_ops = (coords.len() * 100) as u64;
	group.throughput(Throughput::Elements(total_ops));

	// Moka cache hits
	group.bench_function("moka_cache_hits", |b| {
		b.iter(|| {
			rt.block_on(async {
				let mut hit_count = 0;
				for _ in 0..100 {
					for coord in &coords {
						if moka_cache.get(coord).await.is_some() {
							hit_count += 1;
						}
					}
				}
				black_box(hit_count)
			})
		});
	});

	// Mutex cache hits
	group.bench_function("mutex_cache_hits", |b| {
		b.iter(|| {
			rt.block_on(async {
				let mut hit_count = 0;
				for _ in 0..100 {
					for coord in &coords {
						if mutex_cache.get(coord).await.is_some() {
							hit_count += 1;
						}
					}
				}
				black_box(hit_count)
			})
		});
	});

	group.finish();
}

// Benchmark: Concurrent read-heavy workload (realistic tile serving)
fn bench_cache_read_heavy(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("cache_read_heavy");

	let coords: Vec<TileCoord> = (0..8)
		.flat_map(|z| {
			let max = 2u32.pow(u32::from(z));
			(0..max.min(8)).flat_map(move |x| (0..max.min(8)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(100)
		.collect();

	let tile_data = Arc::new(TileData::new(256, 256));

	// Pre-populate moka cache
	let moka_cache: Arc<Cache<TileCoord, Arc<TileData>>> = rt.block_on(async {
		let cache = Cache::builder()
			.max_capacity(512 * 1024 * 1024)
			.weigher(|_k, v: &Arc<TileData>| v.size_bytes())
			.build();

		for coord in &coords {
			cache.insert(*coord, Arc::clone(&tile_data)).await;
		}

		Arc::new(cache)
	});

	// Pre-populate mutex cache
	let mutex_cache = rt.block_on(async {
		let cache = MutexCache::new(100);
		for coord in &coords {
			cache.insert(*coord, Arc::clone(&tile_data)).await;
		}
		Arc::new(cache)
	});

	let total_ops = (coords.len() * 16 * 10) as u64; // 16 threads * 10 reads each
	group.throughput(Throughput::Elements(total_ops));

	// Moka: 16 concurrent readers (simulating tile server load)
	group.bench_function("moka_16_concurrent_readers", |b| {
		b.iter(|| {
			rt.block_on(async {
				let handles: Vec<_> = (0..16)
					.map(|_| {
						let cache = Arc::clone(&moka_cache);
						let coords = coords.clone();
						tokio::spawn(async move {
							for _ in 0..10 {
								for coord in &coords {
									black_box(cache.get(coord).await);
								}
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			});
		});
	});

	// Mutex: 16 concurrent readers
	group.bench_function("mutex_16_concurrent_readers", |b| {
		b.iter(|| {
			rt.block_on(async {
				let handles: Vec<_> = (0..16)
					.map(|_| {
						let cache = Arc::clone(&mutex_cache);
						let coords = coords.clone();
						tokio::spawn(async move {
							for _ in 0..10 {
								for coord in &coords {
									black_box(cache.get(coord).await);
								}
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			});
		});
	});

	group.finish();
}

// Benchmark: DashMap for overview pyramid cache
fn bench_overview_cache(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("overview_pyramid_cache");

	let coords: Vec<TileCoord> = (0..10)
		.flat_map(|z| {
			let max = 2u32.pow(u32::from(z));
			(0..max.min(10)).flat_map(move |x| (0..max.min(10)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(200)
		.collect();

	let tile_data = Arc::new(TileData::new(256, 256));

	group.throughput(Throughput::Elements(coords.len() as u64));

	// DashMap: Concurrent insert + remove pattern (pyramid generation)
	group.bench_function("dashmap_insert_remove_pattern", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache: PyramidCache = Arc::new(DashMap::new());

				// Insert phase: multiple threads adding pyramid data
				let insert_handles: Vec<_> = coords
					.chunks(50)
					.map(|chunk| {
						let cache = Arc::clone(&cache);
						let chunk = chunk.to_vec();
						let tile_data = Arc::clone(&tile_data);
						tokio::spawn(async move {
							for coord in chunk {
								let pyramid_data = vec![(coord, Some(Arc::clone(&tile_data)))];
								cache.insert(coord, pyramid_data);
							}
						})
					})
					.collect();

				for h in insert_handles {
					h.await.unwrap();
				}

				// Remove phase: process and remove entries
				let remove_handles: Vec<_> = coords
					.chunks(50)
					.map(|chunk| {
						let cache = Arc::clone(&cache);
						let chunk = chunk.to_vec();
						tokio::spawn(async move {
							for coord in chunk {
								if let Some((_key, _data)) = cache.remove(&coord) {
									black_box(_data);
								}
							}
						})
					})
					.collect();

				for h in remove_handles {
					h.await.unwrap();
				}

				black_box(cache.len())
			})
		});
	});

	// Old approach: Mutex-based cache with same pattern
	group.bench_function("mutex_insert_remove_pattern", |b| {
		b.iter(|| {
			rt.block_on(async {
				let cache: PyramidMutexCache = Arc::new(AsyncMutex::new(HashMap::new()));

				// Insert phase
				let insert_handles: Vec<_> = coords
					.chunks(50)
					.map(|chunk| {
						let cache = Arc::clone(&cache);
						let chunk = chunk.to_vec();
						let tile_data = Arc::clone(&tile_data);
						tokio::spawn(async move {
							for coord in chunk {
								let pyramid_data = vec![(coord, Some(Arc::clone(&tile_data)))];
								let mut map = cache.lock().await;
								map.insert(coord, pyramid_data);
							}
						})
					})
					.collect();

				for h in insert_handles {
					h.await.unwrap();
				}

				// Remove phase
				let remove_handles: Vec<_> = coords
					.chunks(50)
					.map(|chunk| {
						let cache = Arc::clone(&cache);
						let chunk = chunk.to_vec();
						tokio::spawn(async move {
							for coord in chunk {
								let mut map = cache.lock().await;
								if let Some(data) = map.remove(&coord) {
									black_box(data);
								}
							}
						})
					})
					.collect();

				for h in remove_handles {
					h.await.unwrap();
				}

				black_box(cache.lock().await.len())
			})
		});
	});

	group.finish();
}

criterion_group!(
	benches,
	bench_cache_sequential,
	bench_cache_concurrent_writes,
	bench_cache_hit_performance,
	bench_cache_read_heavy,
	bench_overview_cache,
);

criterion_main!(benches);
