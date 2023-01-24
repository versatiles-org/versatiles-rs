use criterion::{black_box, criterion_group, Criterion};
use log::{set_max_level, LevelFilter};
use opencloudtiles::{
	container::{mbtiles::TileReader, TileReaderTrait},
	helper::*,
};

fn mbtiles_read_vec(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_bbox_tile_vec", |b| {
		let reader = TileReader::new("benches/resources/berlin.mbtiles");
		b.iter(|| {
			black_box(reader.get_bbox_tile_vec(14, &TileBBox::new(8787, 5361, 8818, 5387)));
		})
	});
}

criterion_group!(mbtiles, mbtiles_read_vec);
