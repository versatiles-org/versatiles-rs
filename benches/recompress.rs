use std::time::Duration;

use criterion::{criterion_group, Criterion};
use log::{set_max_level, LevelFilter};
use versatiles::{
	containers::get_reader,
	shared::{Compression, DataConverter, TileBBox},
};

#[allow(dead_code)]
#[tokio::main]
async fn recompress_to_brotli(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	let bbox = TileBBox::new_full(13);
	let mut reader = get_reader("testdata/berlin.mbtiles").await.unwrap();

	let mut vec = reader.get_bbox_tile_vec(&bbox).await.unwrap();
	let n = vec.len() as f32;
	vec = vec
		.into_iter()
		.enumerate()
		.filter_map(|(i, t)| {
			if (i as f32 / n * 100.).fract() >= 100. / n {
				None
			} else {
				Some(t)
			}
		})
		.collect();

	println!("count: {}", vec.len());
	let mut size: usize = 0;
	vec.iter().for_each(|(_c, b)| size += b.len());
	println!("size: {:.2} MB", size as f32 / 1048576.);

	let converter = DataConverter::new_tile_recompressor(
		reader.get_tile_format().unwrap(),
		reader.get_tile_compression().unwrap(),
		reader.get_tile_format().unwrap(),
		&Compression::Brotli,
		true,
	);

	let mut group = c.benchmark_group("sample-size-example");
	group.sample_size(10).measurement_time(Duration::from_secs(20));
	group.bench_function("recompress", |b| {
		let vec = vec.clone();
		let converter = converter.clone();
		b.iter(move || {
			converter.process_vec(vec.clone());
		});
	});
}

criterion_group!(recompress, recompress_to_brotli);
