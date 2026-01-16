#![allow(clippy::cast_possible_truncation)]

use super::*;
use crate::TileBBox;
use futures::TryStreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

fn tc(level: u8, x: u32, y: u32) -> TileCoord {
	TileCoord::new(level, x, y).unwrap()
}

#[tokio::test]
async fn should_flat_map_parallel_and_flatten_results() {
	// Base stream with two coords
	let base = TileStream::from_vec(vec![(tc(1, 0, 0), 10u32), (tc(1, 1, 0), 20u32)]);

	// Each item expands to a sub-stream with two entries
	let flat = base.flat_map_parallel(|coord, val| {
		let out = vec![
			(coord, format!("a:{val}")),
			(tc(coord.level, coord.x, coord.y + 1), format!("b:{val}")),
		];
		Ok(TileStream::from_vec(out))
	});

	// Unwrap Results
	let mut items: Vec<(TileCoord, String)> = flat
		.inner
		.filter_map(|(coord, result)| async move {
			match result {
				Ok(item) => Some(Ok((coord, item))),
				Err(e) => Some(Err(e)),
			}
		})
		.try_collect()
		.await
		.unwrap();

	// Sort for deterministic assertions
	items.sort_by_key(|(c, b)| (c.x, c.y, b.as_str().to_string()));

	assert_eq!(
		items,
		[
			(tc(1, 0, 0), "a:10".into()),
			(tc(1, 0, 1), "b:10".into()),
			(tc(1, 1, 0), "a:20".into()),
			(tc(1, 1, 1), "b:20".into()),
		]
	);
}

#[tokio::test]
async fn should_collect_all_items_from_vec() {
	let tile_data = vec![(tc(0, 0, 0), Blob::from("tile0")), (tc(1, 1, 1), Blob::from("tile1"))];

	let tile_stream = TileStream::from_vec(tile_data.clone());
	let collected = tile_stream.to_vec().await;

	assert_eq!(collected, tile_data);
}

#[tokio::test]
async fn should_iterate_sync_over_items() {
	let tile_data = vec![
		(tc(0, 0, 0), Blob::from("tile0")),
		(tc(1, 1, 1), Blob::from("tile1")),
		(tc(2, 2, 2), Blob::from("tile2")),
	];

	let tile_stream = TileStream::from_vec(tile_data);

	let mut result = vec![];
	tile_stream
		.for_each_sync(|(coord, blob)| {
			result.push(format!("{}, {}", coord.as_json(), blob.as_str()));
		})
		.await;

	assert_eq!(
		result,
		[
			"{\"z\":0,\"x\":0,\"y\":0}, tile0",
			"{\"z\":1,\"x\":1,\"y\":1}, tile1",
			"{\"z\":2,\"x\":2,\"y\":2}, tile2"
		]
	);
}

#[tokio::test]
async fn should_map_coord_properly() {
	let original = TileStream::from_vec(vec![(tc(3, 1, 2), Blob::from("data"))]);

	let mapped = original.map_coord(|coord| tc(coord.level + 1, coord.x * 2, coord.y * 2));

	let items = mapped.to_vec().await;
	assert_eq!(items.len(), 1);
	let (coord, blob) = &items[0];
	assert_eq!(coord.x, 2);
	assert_eq!(coord.y, 4);
	assert_eq!(coord.level, 4);
	assert_eq!(blob.as_str(), "data");
}

#[tokio::test]
async fn should_count_items_with_drain_and_count() {
	let tile_data = vec![
		(tc(0, 0, 0), Blob::from("tile0")),
		(tc(1, 1, 1), Blob::from("tile1")),
		(tc(2, 2, 2), Blob::from("tile2")),
	];

	let tile_stream = TileStream::from_vec(tile_data);
	let count = tile_stream.drain_and_count().await;
	assert_eq!(count, 3, "Should drain exactly 3 items");
}

#[tokio::test]
async fn should_run_for_each_buffered_in_chunks() {
	let tile_data = vec![
		(tc(0, 0, 0), Blob::from("tile0")),
		(tc(1, 1, 1), Blob::from("tile1")),
		(tc(2, 2, 2), Blob::from("tile2")),
	];

	let tile_stream = TileStream::from_vec(tile_data);
	let mut results = Vec::new();

	tile_stream
		.for_each_buffered(2, |chunk| {
			// Each chunk is at most size 2
			results.push(chunk.len());
		})
		.await;

	// Should process a chunk of size 2, then a chunk of size 1
	assert_eq!(results, vec![2, 1]);
}

#[tokio::test]
async fn should_do_parallel_blob_mapping() {
	let tile_data = vec![(tc(0, 0, 0), Blob::from("zero")), (tc(1, 1, 1), Blob::from("one"))];

	// Apply parallel mapping
	let transformed = TileStream::from_vec(tile_data.clone())
		.map_item_parallel(|blob| Ok(Blob::from(format!("mapped-{}", blob.as_str()))));

	// Collect results, unwrapping the Results
	let mut items: Vec<(TileCoord, Blob)> = transformed
		.inner
		.filter_map(|(coord, result)| async move {
			match result {
				Ok(item) => Some(Ok((coord, item))),
				Err(e) => Some(Err(e)),
			}
		})
		.try_collect()
		.await
		.unwrap();
	assert_eq!(items.len(), 2, "Expected two items after mapping");

	// Sort by coordinate level to allow for unordered execution
	items.sort_by_key(|(coord, _)| coord.level);

	// Verify that coordinates are preserved and blobs correctly mapped
	assert_eq!(items[0].0, tc(0, 0, 0));
	assert_eq!(items[0].1.as_str(), "mapped-zero");
	assert_eq!(items[1].0, tc(1, 1, 1));
	assert_eq!(items[1].1.as_str(), "mapped-one");
}

#[tokio::test]
async fn should_parallel_filter_map_blob_correctly() {
	let tile_data = vec![
		(tc(0, 0, 0), Blob::from("keep0")),
		(tc(1, 1, 1), Blob::from("discard1")),
		(tc(2, 2, 2), Blob::from("keep2")),
	];

	let filtered = TileStream::from_vec(tile_data).filter_map_item_parallel(|blob| {
		Ok(if blob.as_str().starts_with("discard") {
			None
		} else {
			Some(Blob::from(format!("kept-{}", blob.as_str())))
		})
	});

	// Collect results, unwrapping the Results
	let items: Vec<(TileCoord, Blob)> = filtered
		.inner
		.filter_map(|(coord, result)| async move {
			match result {
				Ok(item) => Some(Ok((coord, item))),
				Err(e) => Some(Err(e)),
			}
		})
		.try_collect()
		.await
		.unwrap();
	let mut texts = items.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>();
	texts.sort_unstable();
	assert_eq!(texts, ["kept-keep0", "kept-keep2"]);
}

#[tokio::test]
async fn should_construct_empty_stream() {
	let empty = TileStream::<Blob>::empty();
	let collected = empty.to_vec().await;
	assert!(collected.is_empty());
}

#[tokio::test]
async fn should_construct_from_iter_stream() {
	// Create multiple sub-streams
	let substreams = vec![
		Box::pin(async { TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("sub0-0"))]) })
			as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		Box::pin(async { TileStream::from_vec(vec![(tc(1, 1, 1), Blob::from("sub1-1"))]) })
			as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
	];

	// Merge them
	let merged = TileStream::<Blob>::from_streams(stream::iter(substreams));
	let items = merged.to_vec().await;
	assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn should_return_none_if_stream_is_empty() {
	let mut empty = TileStream::<Blob>::empty();
	assert!(empty.next().await.is_none());
}

#[tokio::test]
async fn should_process_async_for_each() {
	let tile_data = vec![(tc(0, 0, 0), Blob::from("async0")), (tc(1, 1, 1), Blob::from("async1"))];

	let s = TileStream::from_vec(tile_data);
	let collected_mutex = Arc::new(Mutex::new(Vec::new()));

	let collected_clone = Arc::clone(&collected_mutex);
	s.for_each_async(move |(coord, blob)| {
		let collected = Arc::clone(&collected_clone);
		async move {
			collected.lock().await.push((coord, blob));
		}
	})
	.await;

	let collected = collected_mutex.lock().await;
	assert_eq!(collected.len(), 2);
	assert_eq!(collected[0].1.as_str(), "async0");
	assert_eq!(collected[1].1.as_str(), "async1");
}

#[tokio::test]
async fn should_filter_by_coord() {
	let stream = TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("z0")), (tc(1, 1, 1), Blob::from("z1"))]);

	let filtered = stream.filter_coord(|coord| async move { coord.level == 0 });
	let items = filtered.to_vec().await;

	assert_eq!(items.len(), 1);
	assert_eq!(items[0].0.level, 0);
	assert_eq!(items[0].1.as_str(), "z0");
}

#[tokio::test]
async fn should_create_from_iter_coord_parallel() {
	let coords = vec![tc(0, 0, 0), tc(1, 1, 1)];

	let stream = TileStream::from_iter_coord_parallel(coords.into_iter(), |coord| {
		Some(Blob::from(format!("v{}", coord.level)))
	});

	let mut items = stream.to_vec().await;
	// Sort for deterministic assertion on unordered parallel output
	items.sort_by_key(|(coord, _)| coord.level);

	assert_eq!(items.len(), 2);
	assert_eq!(items[0].1.as_str(), "v0");
	assert_eq!(items[1].1.as_str(), "v1");
}

#[tokio::test]
async fn should_create_from_bbox_parallel() {
	let bbox = TileBBox::from_min_and_max(4, 0, 0, 2, 2).unwrap();

	let stream = TileStream::from_bbox_parallel(bbox, |coord| {
		Some(Blob::from(format!("v{},{},{}", coord.level, coord.x, coord.y)))
	});

	let mut items = stream.to_vec().await;
	// 3x3 = 9 tiles
	assert_eq!(items.len(), 9);

	// Sort for deterministic assertion on unordered parallel output
	items.sort_by_key(|(coord, _)| (coord.y, coord.x));

	// Verify first and last
	assert_eq!(items[0].1.as_str(), "v4,0,0");
	assert_eq!(items[8].1.as_str(), "v4,2,2");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_from_bbox_parallel_parallelism() {
	let bbox = TileBBox::from_min_and_max(4, 0, 0, 2, 1).unwrap(); // 3x2 = 6 tiles
	let counter = Arc::new(AtomicUsize::new(0));
	let max_parallel = Arc::new(AtomicUsize::new(0));
	let current_parallel = Arc::new(AtomicUsize::new(0));

	let counter_clone = counter.clone();
	let max_parallel_clone = max_parallel.clone();
	let current_parallel_clone = current_parallel.clone();

	let stream = TileStream::from_bbox_parallel(bbox, move |coord| {
		let counter = counter_clone.clone();
		let max_parallel = max_parallel_clone.clone();
		let current_parallel = current_parallel_clone.clone();

		let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
		loop {
			let max = max_parallel.load(Ordering::SeqCst);
			if prev + 1 > max {
				max_parallel.store(prev + 1, Ordering::SeqCst);
			} else {
				break;
			}
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
		current_parallel.fetch_sub(1, Ordering::SeqCst);
		counter.fetch_add(1, Ordering::SeqCst);
		Some(Blob::from(format!("{}", coord.level)))
	});

	let results = stream.to_vec().await;
	assert_eq!(results.len(), 6);
	assert_eq!(counter.load(Ordering::SeqCst), 6);
	assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
}

#[tokio::test]
async fn should_create_from_bbox_async_parallel() {
	let bbox = TileBBox::from_min_and_max(4, 0, 0, 2, 2).unwrap();

	let stream = TileStream::from_bbox_async_parallel(bbox, |coord| async move {
		Some((coord, Blob::from(format!("v{},{},{}", coord.level, coord.x, coord.y))))
	});

	let mut items = stream.to_vec().await;
	// 3x3 = 9 tiles
	assert_eq!(items.len(), 9);

	// Sort for deterministic assertion on unordered parallel output
	items.sort_by_key(|(coord, _)| (coord.y, coord.x));

	// Verify first and last
	assert_eq!(items[0].1.as_str(), "v4,0,0");
	assert_eq!(items[8].1.as_str(), "v4,2,2");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_from_bbox_async_parallel_parallelism() {
	let bbox = TileBBox::from_min_and_max(4, 0, 0, 2, 1).unwrap(); // 3x2 = 6 tiles
	let counter = Arc::new(AtomicUsize::new(0));
	let max_parallel = Arc::new(AtomicUsize::new(0));
	let current_parallel = Arc::new(AtomicUsize::new(0));

	let counter_clone = counter.clone();
	let max_parallel_clone = max_parallel.clone();
	let current_parallel_clone = current_parallel.clone();

	let stream = TileStream::from_bbox_async_parallel(bbox, move |coord| {
		let counter = counter_clone.clone();
		let max_parallel = max_parallel_clone.clone();
		let current_parallel = current_parallel_clone.clone();

		async move {
			let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
			loop {
				let max = max_parallel.load(Ordering::SeqCst);
				if prev + 1 > max {
					max_parallel.store(prev + 1, Ordering::SeqCst);
				} else {
					break;
				}
			}
			tokio::time::sleep(std::time::Duration::from_millis(10)).await;
			current_parallel.fetch_sub(1, Ordering::SeqCst);
			counter.fetch_add(1, Ordering::SeqCst);
			Some((coord, Blob::from(format!("{}", coord.level))))
		}
	});

	let results = stream.to_vec().await;
	assert_eq!(results.len(), 6);
	assert_eq!(counter.load(Ordering::SeqCst), 6);
	assert!(max_parallel.load(Ordering::SeqCst) > 1, "Expected parallel execution");
}

#[tokio::test]
async fn should_create_from_coord_vec_async() {
	let coords = vec![tc(0, 0, 0), tc(1, 1, 1)];

	let stream = TileStream::from_coord_vec_async(coords, |coord| async move {
		if coord.level == 0 {
			Some((coord, Blob::from("keep")))
		} else {
			None
		}
	});

	let items = stream.to_vec().await;
	assert_eq!(items.len(), 1);
	assert_eq!(items[0].0.level, 0);
	assert_eq!(items[0].1.as_str(), "keep");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_map_item_parallel_parallelism() {
	let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());
	let counter = Arc::new(AtomicUsize::new(0));
	let max_parallel = Arc::new(AtomicUsize::new(0));
	let current_parallel = Arc::new(AtomicUsize::new(0));

	let counter_clone = counter.clone();
	let max_parallel_clone = max_parallel.clone();
	let current_parallel_clone = current_parallel.clone();

	let stream = stream.map_item_parallel(move |item| {
		let counter = counter_clone.clone();
		let max_parallel = max_parallel_clone.clone();
		let current_parallel = current_parallel_clone.clone();

		let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
		loop {
			let max = max_parallel.load(Ordering::SeqCst);
			if prev + 1 > max {
				max_parallel.store(prev + 1, Ordering::SeqCst);
			} else {
				break;
			}
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
		current_parallel.fetch_sub(1, Ordering::SeqCst);
		counter.fetch_add(1, Ordering::SeqCst);
		Ok(item)
	});

	// Collect results, unwrapping the Results
	let results: Vec<(TileCoord, u32)> = stream
		.inner
		.filter_map(|(coord, result)| async move {
			match result {
				Ok(item) => Some(Ok((coord, item))),
				Err(e) => Some(Err(e)),
			}
		})
		.try_collect()
		.await
		.unwrap();
	assert_eq!(results.len(), 6);
	assert_eq!(counter.load(Ordering::SeqCst), 6);
	assert!(max_parallel.load(Ordering::SeqCst) > 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_filter_map_item_parallel_parallelism() {
	let stream = TileStream::from_vec(
		vec![Some(1), None, Some(3), None, Some(5), None]
			.into_iter()
			.enumerate()
			.map(|(i, v)| (tc(12, i as u32, 0), v))
			.collect::<Vec<_>>(),
	);
	let counter = Arc::new(AtomicUsize::new(0));
	let max_parallel = Arc::new(AtomicUsize::new(0));
	let current_parallel = Arc::new(AtomicUsize::new(0));

	let counter_clone = counter.clone();
	let max_parallel_clone = max_parallel.clone();
	let current_parallel_clone = current_parallel.clone();

	let stream = stream.filter_map_item_parallel(move |item| {
		let counter = counter_clone.clone();
		let max_parallel = max_parallel_clone.clone();
		let current_parallel = current_parallel_clone.clone();

		let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
		loop {
			let max = max_parallel.load(Ordering::SeqCst);
			if prev + 1 > max {
				max_parallel.store(prev + 1, Ordering::SeqCst);
			} else {
				break;
			}
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
		current_parallel.fetch_sub(1, Ordering::SeqCst);
		counter.fetch_add(1, Ordering::SeqCst);
		Ok(item)
	});

	// Collect results, unwrapping the Results
	let results: Vec<(TileCoord, u32)> = stream
		.inner
		.filter_map(|(coord, result)| async move {
			match result {
				Ok(item) => Some(Ok((coord, item))),
				Err(e) => Some(Err(e)),
			}
		})
		.try_collect()
		.await
		.unwrap();
	assert_eq!(results.len(), 3);
	assert_eq!(counter.load(Ordering::SeqCst), 6);
	assert!(max_parallel.load(Ordering::SeqCst) > 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_for_each_async_parallel_parallelism() {
	let stream = TileStream::from_vec((1..=6).map(|i| (tc(12, i, 0), i)).collect::<Vec<_>>());
	let counter = Arc::new(AtomicUsize::new(0));
	let max_parallel = Arc::new(AtomicUsize::new(0));
	let current_parallel = Arc::new(AtomicUsize::new(0));

	let counter_clone = counter.clone();
	let max_parallel_clone = max_parallel.clone();
	let current_parallel_clone = current_parallel.clone();

	stream
		.for_each_async_parallel(move |_item| {
			let counter = counter_clone.clone();
			let max_parallel = max_parallel_clone.clone();
			let current_parallel = current_parallel_clone.clone();

			async move {
				let prev = current_parallel.fetch_add(1, Ordering::SeqCst);
				loop {
					let max = max_parallel.load(Ordering::SeqCst);
					if prev + 1 > max {
						max_parallel.store(prev + 1, Ordering::SeqCst);
					} else {
						break;
					}
				}
				tokio::time::sleep(std::time::Duration::from_millis(10)).await;
				current_parallel.fetch_sub(1, Ordering::SeqCst);
				counter.fetch_add(1, Ordering::SeqCst);
			}
		})
		.await;

	assert_eq!(counter.load(Ordering::SeqCst), 6);
	assert!(max_parallel.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn should_merge_streams_with_large_cores_per_task() {
	// cores_per_task larger than CPU count should still work (limit clamped to 1)
	let substreams = vec![
		Box::pin(async { TileStream::from_vec(vec![(tc(0, 0, 0), Blob::from("a"))]) })
			as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
		Box::pin(async { TileStream::from_vec(vec![(tc(1, 1, 1), Blob::from("b"))]) })
			as Pin<Box<dyn Future<Output = TileStream<'static>> + Send>>,
	];
	let merged = TileStream::<Blob>::from_streams(stream::iter(substreams));
	let items = merged.to_vec().await;
	assert_eq!(items.len(), 2);
}
