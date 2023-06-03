use criterion::{black_box, criterion_group, Criterion};
use futures::{executor::block_on, StreamExt};
use log::{set_max_level, LevelFilter};
use versatiles::{containers::get_reader, shared::TileBBox};

fn mbtiles_read_vec(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_bbox_tile_stream", |b| {
		let mut reader = block_on(get_reader("testdata/berlin.mbtiles")).unwrap();
		b.iter(|| {
			block_on(async {
				let bbox = TileBBox::new(14, 8787, 5361, 8818, 5387);
				let stream = reader.get_bbox_tile_stream(&bbox).await;
				let _count = stream.count().await;
			});
		})
	});
}

criterion_group!(mbtiles, mbtiles_read_vec);
