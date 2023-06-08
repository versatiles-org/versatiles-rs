use criterion::{async_executor::FuturesExecutor, criterion_group, Criterion};
use futures::StreamExt;
use log::{set_max_level, LevelFilter};
use versatiles::{containers::get_reader, shared::TileBBox};

#[tokio::main]
async fn mbtiles_read_vec(c: &mut Criterion) {
	set_max_level(LevelFilter::Warn);

	c.bench_function("get_bbox_tile_iterator", |b| {
		let bbox = TileBBox::new(14, 8787, 5361, 8818, 5387);
		b.to_async(FuturesExecutor).iter(|| async {
			let mut reader = get_reader("testdata/berlin.mbtiles").await.unwrap();
			let stream = reader.get_bbox_tile_iterator(&bbox).await;
			let _count = stream.count().await;
		});
	});
}

criterion_group!(mbtiles, mbtiles_read_vec);
