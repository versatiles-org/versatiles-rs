use criterion::criterion_main;

mod benchmarks;

criterion_main! {
	benchmarks::mbtiles::mbtiles,
	benchmarks::cloudtiles::cloudtiles,
}
