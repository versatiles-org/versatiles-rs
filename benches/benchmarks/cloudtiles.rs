use criterion::{black_box, criterion_group, Criterion};
use log::{set_max_level, LevelFilter};
use opencloudtiles::{
	container::{cloudtiles::TileReader, TileReaderTrait},
	helper::*,
};
use rand::seq::SliceRandom;

fn cloudtiles_read(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_tile_data", |b| {
		let reader = TileReader::new("benches/resources/berlin.cloudtiles");
		let coords: Vec<TileCoord3> = reader
			.get_parameters()
			.get_bbox_pyramide()
			.iter_tile_indexes()
			.collect();

		b.iter(|| {
			let coord = coords.choose(&mut rand::thread_rng()).unwrap();
			black_box(reader.get_tile_data(coord));
		})
	});
}

criterion_group!(cloudtiles, cloudtiles_read);
