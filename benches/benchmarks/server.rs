use criterion::{black_box, criterion_group, Criterion};
use log::{set_max_level, LevelFilter};

fn bench_server(c: &mut Criterion) {
	let mut group = c.benchmark_group("test_server");

	set_max_level(LevelFilter::Warn);
	let args = opencloudtiles::tools::serve::Subcommand {
		sources: vec!["benches/resources/berlin.mbtiles".to_string()],
		port: 8080,
		static_folder: None,
		static_tar: None,
	};
	let _server = opencloudtiles::tools::serve::run(&args);

	group.sample_size(50);
	group.bench_function("tile_request", |b| {
		b.iter(|| async {
			let _resp = black_box(
				reqwest::blocking::get("http://localhost:8080/tiles/berlin/0/0/0")
					.unwrap()
					.bytes(),
			);
			// Client.
			// let coord = coords.choose(&mut rand::thread_rng()).unwrap();
			// black_box(reader.get_tile_data(coord));
		})
	});
}

criterion_group!(server, bench_server);
