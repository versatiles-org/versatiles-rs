mod mbtiles;

use criterion::criterion_main;

criterion_main! {
	mbtiles::mbtiles,
}
