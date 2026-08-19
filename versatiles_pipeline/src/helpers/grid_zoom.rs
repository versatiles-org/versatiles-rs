//! Deriving the lowest zoom level a fixed-size grid can be served at.
//!
//! A grid source keeps its cell size constant across zoom levels — that is what
//! makes a cell id stable enough to join data against. The consequence is that
//! low zoom levels are unusable: a 1 km grid at zoom 0 would put every cell on
//! Earth into a single tile.
//!
//! Unlike [`auto_max_zoom`](versatiles_geometry::feature_import::auto_max_zoom),
//! which has to guess from the median feature size, the cell size here is known
//! exactly, so the lowest usable zoom is a closed form rather than a heuristic.

use versatiles_core::WORLD_SIZE;

/// Highest zoom a derived minimum is allowed to reach.
///
/// TileJSON caps `minzoom`/`maxzoom` at 30, and a grid whose cells are so small
/// that they only become servable past that is a mistake worth reporting rather
/// than silently clamping — callers check the result against their own maximum.
const MAX_ZOOM: u8 = 30;

/// The lowest zoom level at which a tile holds at most `max_cells_per_tile` cells.
///
/// At zoom `z` a tile spans `WORLD_SIZE / 2^z` mercator meters, so it holds
/// `(WORLD_SIZE / 2^z / cell_size)²` cells. Solving that for the smallest `z`
/// where the count stays within budget:
///
/// ```text
/// z = ceil( log2( WORLD_SIZE / (cell_size · √max_cells_per_tile) ) )
/// ```
///
/// `cell_size` is measured in **mercator** meters rather than on the ground,
/// because that is the space tiles are cut in. Callers projecting from another
/// CRS should measure a cell where mercator makes it smallest — nearest the
/// equator — so the resulting bound is the conservative one.
#[must_use]
pub fn min_zoom_for_cell_size(cell_size_mercator: f64, max_cells_per_tile: u32) -> u8 {
	if !cell_size_mercator.is_finite() || cell_size_mercator <= 0.0 || max_cells_per_tile == 0 {
		return MAX_ZOOM;
	}

	let cells_per_axis = f64::from(max_cells_per_tile).sqrt();
	let ratio = WORLD_SIZE / (cell_size_mercator * cells_per_axis);
	if ratio <= 1.0 {
		// One tile at zoom 0 already holds few enough cells.
		return 0;
	}

	let zoom = ratio.log2().ceil();
	if zoom >= f64::from(MAX_ZOOM) {
		MAX_ZOOM
	} else {
		// Safe: bounded by MAX_ZOOM above and > 0 by the ratio check.
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		let zoom = zoom as u8;
		zoom
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn one_kilometer_grid() {
		// 40075 km / 2^11 ≈ 19.6 km per tile → ~383 cells; zoom 10 would be ~1530.
		assert_eq!(min_zoom_for_cell_size(1000.0, 1024), 11);
		assert_eq!(min_zoom_for_cell_size(1000.0, 4096), 10);
		assert_eq!(min_zoom_for_cell_size(1000.0, 256), 12);
	}

	#[test]
	fn larger_cells_need_less_zoom() {
		assert_eq!(min_zoom_for_cell_size(100_000.0, 1024), 4);
		assert_eq!(min_zoom_for_cell_size(1_000_000.0, 1024), 1);
	}

	#[test]
	fn a_cell_bigger_than_the_world_starts_at_zero() {
		assert_eq!(min_zoom_for_cell_size(WORLD_SIZE, 1), 0);
		assert_eq!(min_zoom_for_cell_size(WORLD_SIZE * 4.0, 1), 0);
	}

	#[test]
	fn the_budget_is_respected_at_the_returned_zoom() {
		for cell in [10.0, 250.0, 1000.0, 25_000.0] {
			for budget in [16u32, 256, 1024, 4096] {
				let z = min_zoom_for_cell_size(cell, budget);
				let tile_size = WORLD_SIZE / 2f64.powi(i32::from(z));
				let cells = (tile_size / cell).powi(2);
				assert!(
					cells <= f64::from(budget),
					"cell {cell} budget {budget}: {cells} cells at z{z}"
				);
				if z > 0 {
					// ...and one level lower would not be.
					let bigger = (tile_size * 2.0 / cell).powi(2);
					assert!(
						bigger > f64::from(budget),
						"cell {cell} budget {budget}: z{z} is not minimal"
					);
				}
			}
		}
	}

	#[test]
	fn nonsense_input_is_pushed_to_the_maximum() {
		assert_eq!(min_zoom_for_cell_size(0.0, 1024), MAX_ZOOM);
		assert_eq!(min_zoom_for_cell_size(-1.0, 1024), MAX_ZOOM);
		assert_eq!(min_zoom_for_cell_size(f64::NAN, 1024), MAX_ZOOM);
		assert_eq!(min_zoom_for_cell_size(1000.0, 0), MAX_ZOOM);
	}
}
