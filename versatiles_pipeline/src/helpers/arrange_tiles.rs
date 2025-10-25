use versatiles_container::Tile;
use versatiles_core::TileCoord;

pub fn arrange_tiles<T: ToString>(tiles: Vec<(TileCoord, Tile)>, cb: impl Fn(Tile) -> T) -> Vec<String> {
	use versatiles_core::TileBBox;

	let mut bbox = TileBBox::new_empty(tiles.first().unwrap().0.level).unwrap();
	tiles.iter().for_each(|t| bbox.include(t.0.x, t.0.y));

	let mut result: Vec<Vec<String>> = (0..bbox.height())
		.map(|_| (0..bbox.width()).map(|_| String::from("❌")).collect())
		.collect();

	for (coord, item) in tiles.into_iter() {
		let x = (coord.x - bbox.x_min()) as usize;
		let y = (coord.y - bbox.y_min()) as usize;
		result[y][x] = cb(item).to_string();
	}
	result.into_iter().map(|r| r.join(" ")).collect::<Vec<String>>()
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles_core::{Blob, TileCompression::Uncompressed, TileFormat::BIN};

	#[test]
	fn test_arrange_tiles() {
		let tiles = vec![(0, 0, "a"), (1, 0, "b"), (0, 1, "c")]
			.into_iter()
			.map(|(x, y, v)| {
				(
					TileCoord::new(8, x, y).unwrap(),
					Tile::from_blob(Blob::from(v), Uncompressed, BIN),
				)
			})
			.collect();

		let arranged = arrange_tiles(tiles, |tile| tile.into_blob(Uncompressed).as_str().to_string());
		assert_eq!(arranged, ["a b", "c ❌"]);
	}
}
