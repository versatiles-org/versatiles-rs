use core::time;
use criterion::{black_box, criterion_group, Criterion};
use futures::executor::block_on;
use rand::seq::SliceRandom;
use std::thread;
use versatiles::{
	container::{mbtiles::TileReader, TileReaderTrait},
	helper::TileCoord3,
};

fn bench_server(c: &mut Criterion) {
	let mut group = c.benchmark_group("test_server");

	let reader = block_on(TileReader::new("benches/resources/berlin.mbtiles"));
	let coords: Vec<TileCoord3> = reader
		.get_parameters()
		.get_bbox_pyramide()
		.iter_tile_indexes()
		.collect();
	drop(reader);

	let args = versatiles::tools::serve::Subcommand {
		sources: vec!["benches/resources/berlin.mbtiles".to_string()],
		ip: "127.0.0.1".to_owned(),
		port: 8080,
		static_content: Vec::new(),
	};
	thread::spawn(move || versatiles::tools::serve::run(&args));

	thread::sleep(time::Duration::from_secs(1));

	group.sample_size(50);
	group.bench_function("tile_request", |b| {
		b.iter(|| {
			let coord = coords.choose(&mut rand::thread_rng()).unwrap();
			let url = format!("http://127.0.0.1:8080/tiles/berlin/{}/{}/{}", coord.z, coord.y, coord.x);

			let _resp = black_box(reqwest::blocking::get(url).unwrap().text().unwrap());
		})
	});
}

criterion_group!(server, bench_server);
