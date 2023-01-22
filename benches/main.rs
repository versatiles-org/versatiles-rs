use criterion::{criterion_group, criterion_main, Criterion};
use log::{set_max_level, LevelFilter};
use opencloudtiles::{
	container::{mbtiles::TileReader, TileReaderTrait},
	helper::*,
};

/*
fn mbtiles_new(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);
	c.bench_function("TileReader::new mbtiles", |b| {
		b.iter(|| {
			TileReader::new("benches/ressources/2023-01-eu-de-be.mbtiles");
		});
	});
}
criterion_group!(benches, mbtiles_new);
*/

fn mbtiles_read_vec(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);
	let reader = TileReader::new("benches/ressources/2023-01-eu-de-be.mbtiles");

	c.bench_function("get_bbox_tile_vec", |b| {
		b.iter(|| {
			reader.get_bbox_tile_vec(14, &TileBBox::new(8787, 5361, 8818, 5387));
		})
	});
}

criterion_group!(benches, mbtiles_read_vec);
criterion_main!(benches);
