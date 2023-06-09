mod mbtiles;
mod recompress;

use criterion::criterion_main;

criterion_main! {
	//mbtiles::mbtiles,
	recompress::recompress,
}
