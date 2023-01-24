use criterion::{black_box, criterion_group, Criterion};
use log::{set_max_level, LevelFilter};
use opencloudtiles::{
	container::{cloudtiles::TileReader, TileReaderTrait},
	helper::*,
};
use rand::seq::SliceRandom;

fn cloudtiles_read(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);
	let reader = TileReader::new("benches/ressources/2023-01-eu-de-be.cloudtiles");
	let coords: Vec<TileCoord3> = reader
		.get_parameters()
		.get_bbox_pyramide()
		.iter_tile_indexes()
		.collect();

	c.bench_function("get_tile_data", |b| {
		b.iter(|| {
			let coord = coords.choose(&mut rand::thread_rng()).unwrap();
			black_box(reader.get_tile_data(coord));
		})
	});
}

criterion_group!(cloudtiles, cloudtiles_read);
