use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use futures::stream::{self, StreamExt};
use std::hint::black_box;
use std::sync::Arc;
use tokio::runtime::Runtime;
use versatiles_core::{ConcurrencyLimits, TileCoord, TileStream};

// Benchmark: Concurrency limits for different workload types
fn bench_concurrency_limits(c: &mut Criterion) {
	let mut group = c.benchmark_group("concurrency_limits");

	let limits = ConcurrencyLimits::default();
	let cpu_count = ConcurrencyLimits::cpu_count();

	group.bench_function("cpu_count", |b| b.iter(|| black_box(ConcurrencyLimits::cpu_count())));

	group.bench_function("default_limits", |b| b.iter(|| black_box(ConcurrencyLimits::default())));

	// Verify our assumptions
	assert_eq!(limits.cpu_bound, cpu_count);
	assert_eq!(limits.io_bound, cpu_count * 3);
	assert_eq!(limits.mixed, cpu_count + (cpu_count / 2));

	group.finish();
}

// Benchmark: Tile stream parallel processing with different concurrency
fn bench_tile_stream_concurrency(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("tile_stream_concurrency");

	// Create test data: 1000 tiles
	let coords: Vec<TileCoord> = (0..10)
		.flat_map(|z| {
			let max = 2u32.pow(z as u32);
			(0..max.min(10)).flat_map(move |x| (0..max.min(10)).filter_map(move |y| TileCoord::new(z, x, y).ok()))
		})
		.take(1000)
		.collect();

	group.throughput(Throughput::Elements(coords.len() as u64));

	// CPU-bound workload simulation
	let cpu_work = Arc::new(|coord: TileCoord| {
		// Simulate CPU work: compute hash
		let hash = (coord.level as u64) * 1000000 + (coord.x as u64) * 1000 + (coord.y as u64);
		for _ in 0..100 {
			black_box(hash.wrapping_mul(31).wrapping_add(17));
		}
		Some(hash)
	});

	group.bench_function("cpu_bound_parallel", |b| {
		b.iter(|| {
			let coords_clone = coords.clone();
			let work = Arc::clone(&cpu_work);
			rt.block_on(async {
				let stream = TileStream::from_iter_coord_parallel(coords_clone.into_iter(), move |c| work(c));
				stream.drain_and_count().await
			})
		});
	});

	group.finish();
}

// Benchmark: Stream buffering with different limits
fn bench_stream_buffering(c: &mut Criterion) {
	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("stream_buffering");

	let limits = ConcurrencyLimits::default();
	let item_count = 1000u64;

	group.throughput(Throughput::Elements(item_count));

	// I/O-bound simulation
	group.bench_function("io_bound_3x_cpu", |b| {
		b.iter(|| {
			rt.block_on(async {
				stream::iter(0..item_count)
					.map(|i| async move {
						// Simulate I/O wait
						tokio::task::yield_now().await;
						black_box(i)
					})
					.buffer_unordered(limits.io_bound)
					.count()
					.await
			})
		});
	});

	// CPU-bound simulation
	group.bench_function("cpu_bound_1x_cpu", |b| {
		b.iter(|| {
			rt.block_on(async {
				stream::iter(0..item_count)
					.map(|i| {
						tokio::task::spawn_blocking(move || {
							// Simulate CPU work
							let mut val = i;
							for _ in 0..100 {
								val = black_box(val.wrapping_mul(31).wrapping_add(17));
							}
							val
						})
					})
					.buffer_unordered(limits.cpu_bound)
					.count()
					.await
			})
		});
	});

	// Mixed workload
	group.bench_function("mixed_1.5x_cpu", |b| {
		b.iter(|| {
			rt.block_on(async {
				stream::iter(0..item_count)
					.map(|i| async move {
						tokio::task::yield_now().await; // I/O
						black_box(i * 2) // CPU
					})
					.buffer_unordered(limits.mixed)
					.count()
					.await
			})
		});
	});

	group.finish();
}

// Benchmark: Concurrent hash map lookups (simulating DashMap)
fn bench_concurrent_lookups(c: &mut Criterion) {
	use dashmap::DashMap;
	use std::collections::HashMap;
	use tokio::sync::RwLock;

	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("concurrent_lookups");

	// Prepare test data
	let data: Vec<(String, u64)> = (0..1000).map(|i| (format!("key_{}", i), i as u64)).collect();

	// DashMap (lock-free)
	let dashmap = Arc::new(DashMap::new());
	for (k, v) in &data {
		dashmap.insert(k.clone(), *v);
	}

	// RwLock<HashMap> (locked)
	let hashmap = Arc::new(RwLock::new({
		let mut map = HashMap::new();
		for (k, v) in &data {
			map.insert(k.clone(), *v);
		}
		map
	}));

	let lookups = 10000u64;
	group.throughput(Throughput::Elements(lookups));

	// DashMap concurrent reads
	group.bench_function("dashmap_concurrent_reads", |b| {
		b.iter(|| {
			let map = Arc::clone(&dashmap);
			rt.block_on(async move {
				let handles: Vec<_> = (0..100)
					.map(|_| {
						let m = Arc::clone(&map);
						tokio::spawn(async move {
							for i in 0..100 {
								let key = format!("key_{}", i % 1000);
								black_box(m.get(&key));
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			})
		});
	});

	// RwLock<HashMap> concurrent reads
	group.bench_function("rwlock_concurrent_reads", |b| {
		b.iter(|| {
			let map = Arc::clone(&hashmap);
			rt.block_on(async move {
				let handles: Vec<_> = (0..100)
					.map(|_| {
						let m = Arc::clone(&map);
						tokio::spawn(async move {
							for i in 0..100 {
								let key = format!("key_{}", i % 1000);
								let map = m.read().await;
								black_box(map.get(&key));
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			})
		});
	});

	group.finish();
}

// Benchmark: ArcSwap vs RwLock for read-heavy workloads
fn bench_arcswap_vs_rwlock(c: &mut Criterion) {
	use arc_swap::ArcSwap;
	use tokio::sync::RwLock;

	let rt = Runtime::new().unwrap();
	let mut group = c.benchmark_group("arcswap_vs_rwlock");

	let data: Vec<String> = (0..100).map(|i| format!("item_{}", i)).collect();

	// ArcSwap
	let arcswap = Arc::new(ArcSwap::from_pointee(data.clone()));

	// RwLock
	let rwlock = Arc::new(RwLock::new(data.clone()));

	let reads = 10000u64;
	group.throughput(Throughput::Elements(reads));

	// ArcSwap concurrent reads
	group.bench_function("arcswap_concurrent_reads", |b| {
		b.iter(|| {
			let swap = Arc::clone(&arcswap);
			rt.block_on(async move {
				let handles: Vec<_> = (0..100)
					.map(|_| {
						let s = Arc::clone(&swap);
						tokio::spawn(async move {
							for _ in 0..100 {
								let data = s.load();
								black_box(data.len());
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			})
		});
	});

	// RwLock concurrent reads
	group.bench_function("rwlock_concurrent_reads", |b| {
		b.iter(|| {
			let lock = Arc::clone(&rwlock);
			rt.block_on(async move {
				let handles: Vec<_> = (0..100)
					.map(|_| {
						let l = Arc::clone(&lock);
						tokio::spawn(async move {
							for _ in 0..100 {
								let data = l.read().await;
								black_box(data.len());
							}
						})
					})
					.collect();

				for h in handles {
					h.await.unwrap();
				}
			})
		});
	});

	group.finish();
}

// Benchmark: parking_lot::Mutex vs std::sync::Mutex
fn bench_parking_lot_mutex(c: &mut Criterion) {
	use parking_lot::Mutex as ParkingMutex;
	use std::sync::Mutex as StdMutex;

	let mut group = c.benchmark_group("mutex_comparison");

	let parking = Arc::new(ParkingMutex::new(0u64));
	let std = Arc::new(StdMutex::new(0u64));

	let ops = 1000u64;
	group.throughput(Throughput::Elements(ops));

	// parking_lot::Mutex
	group.bench_function("parking_lot_mutex", |b| {
		b.iter(|| {
			let m = Arc::clone(&parking);
			for _ in 0..ops {
				let mut val = m.lock();
				*val = black_box(*val + 1);
			}
		});
	});

	// std::sync::Mutex
	group.bench_function("std_sync_mutex", |b| {
		b.iter(|| {
			let m = Arc::clone(&std);
			for _ in 0..ops {
				let mut val = m.lock().unwrap();
				*val = black_box(*val + 1);
			}
		});
	});

	group.finish();
}

criterion_group!(
	benches,
	bench_concurrency_limits,
	bench_tile_stream_concurrency,
	bench_stream_buffering,
	bench_concurrent_lookups,
	bench_arcswap_vs_rwlock,
	bench_parking_lot_mutex,
);

criterion_main!(benches);
