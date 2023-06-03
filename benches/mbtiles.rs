use criterion::{black_box, criterion_group, Criterion};
use futures::executor::block_on;
use log::{set_max_level, LevelFilter};
use versatiles::{containers::get_reader, shared::TileBBox};

fn mbtiles_read_vec(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_bbox_tile_vec", |b| {
		let mut reader = block_on(get_reader("benches/ressources/berlin.mbtiles")).unwrap();
		b.iter(|| {
			black_box(block_on(
				reader.get_bbox_tile_stream(&TileBBox::new(14, 8787, 5361, 8818, 5387)),
			));
		})
	});
}

criterion_group!(mbtiles, mbtiles_read_vec);
