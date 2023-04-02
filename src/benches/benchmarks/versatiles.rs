use criterion::{black_box, criterion_group, Criterion};
use futures::executor::block_on;
use log::{set_max_level, LevelFilter};
use rand::{seq::SliceRandom, thread_rng};
use versatiles_container::get_reader;
use versatiles_shared::TileCoord3;

fn versatiles_read(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_tile_data", |b| {
		let reader = block_on(get_reader("benches/resources/berlin.versatiles"));
		let coords: Vec<TileCoord3> = reader
			.get_parameters()
			.get_bbox_pyramide()
			.iter_tile_indexes()
			.collect();

		b.iter(|| {
			let coord = coords.choose(&mut thread_rng()).unwrap();
			black_box(reader.get_tile_data(coord));
		})
	});
}

criterion_group!(versatiles, versatiles_read);
