//! Reproduction of issue #230: TilesConvertReader answering tile() and
//! tile_stream() differently when flip_y and swap_xy are combined.
use versatiles_container::{TileSource, TilesConvertReader, TilesConverterParameters, TilesRuntime};
use versatiles_core::{TileBBox, TileCompression};

async fn mismatches(flip_y: bool, swap_xy: bool) -> (usize, usize) {
	let runtime = TilesRuntime::new_silent();
	let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await.unwrap();
	let cp = TilesConverterParameters {
		flip_y,
		swap_xy,
		..Default::default()
	};
	let conv = TilesConvertReader::new_from_reader(reader, cp).await.unwrap();

	let pyramid = conv.tile_pyramid().await.unwrap();
	let bbox: TileBBox = pyramid.level_ref(14).to_bbox();
	let mut stream = conv.tile_stream(bbox).await.unwrap();

	let (mut checked, mut bad) = (0usize, 0usize);
	while let Some((coord, t)) = stream.next().await {
		let a = t.into_blob(&TileCompression::Uncompressed).ok();
		let b = conv
			.tile(&coord)
			.await
			.unwrap()
			.and_then(|t| t.into_blob(&TileCompression::Uncompressed).ok());
		if a != b {
			bad += 1;
		}
		checked += 1;
		if checked >= 40 {
			break;
		}
	}
	(checked, bad)
}

#[tokio::test]
async fn tile_and_tile_stream_agree_for_every_flag_combination() {
	for (flip_y, swap_xy) in [(false, false), (true, false), (false, true), (true, true)] {
		let (checked, bad) = mismatches(flip_y, swap_xy).await;
		assert!(checked > 0, "flip_y={flip_y} swap_xy={swap_xy}: nothing to compare");
		assert_eq!(bad, 0, "flip_y={flip_y} swap_xy={swap_xy}: {bad}/{checked} mismatches");
	}
}
