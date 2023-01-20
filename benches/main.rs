use criterion::{criterion_group, criterion_main, Criterion};
use opencloudtiles::container::mbtiles::TileReader;
use opencloudtiles::container::TileReaderTrait;
use opencloudtiles::helper::*;

fn mbtiles_read_speed(c: &mut Criterion) {
	let reader = TileReader::new("tests/ressources/2023-01-eu-de-be.mbtiles");

	c.bench_function("get_bbox_tile_vec", |b| {
		b.iter(|| {
			let vec = reader.get_bbox_tile_vec(14, &TileBBox::new(8787, 5361, 8818, 5387));
			println!("{}", vec.len());
		})
	});
}

criterion_group!(benches, mbtiles_read_speed);
criterion_main!(benches);
