mod benchmarks;

use criterion::criterion_main;

criterion_main! {
	benchmarks::mbtiles::mbtiles,
	benchmarks::versatiles::versatiles,
	benchmarks::server::server,
}
