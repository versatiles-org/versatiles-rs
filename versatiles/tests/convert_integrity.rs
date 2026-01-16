//! E2E tests for tile data integrity after format conversions.
//!
//! These tests verify that tile data is preserved byte-for-byte when converting
//! between container formats using the CLI.

mod test_utilities;

use std::sync::Arc;
use tempfile::TempDir;
use test_utilities::*;
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::{TileCompression, TileCoord};

/// Helper to compare tiles from two readers at a given coordinate.
async fn assert_tiles_equal(
	source_reader: &Arc<Box<dyn TileSource>>,
	output_reader: &Arc<Box<dyn TileSource>>,
	coord: &TileCoord,
) {
	let source_tile = source_reader.get_tile(coord).await.unwrap();
	let output_tile = output_reader.get_tile(coord).await.unwrap();

	match (source_tile, output_tile) {
		(Some(mut src), Some(mut out)) => {
			// Get uncompressed blobs for comparison
			let src_blob = src.as_blob(TileCompression::Uncompressed).unwrap();
			let out_blob = out.as_blob(TileCompression::Uncompressed).unwrap();
			assert_eq!(
				src_blob.as_slice(),
				out_blob.as_slice(),
				"Tile data mismatch at level={}, x={}, y={}",
				coord.level,
				coord.x,
				coord.y
			);
		}
		(None, None) => {
			// Both missing is fine
		}
		(Some(_), None) => {
			panic!(
				"Tile missing in output at level={}, x={}, y={}",
				coord.level, coord.x, coord.y
			);
		}
		(None, Some(_)) => {
			panic!(
				"Unexpected tile in output at level={}, x={}, y={}",
				coord.level, coord.x, coord.y
			);
		}
	}
}

/// Test that tile data is preserved when converting mbtiles to versatiles.
#[tokio::test]
async fn e2e_tile_integrity_mbtiles_to_versatiles() {
	let input = get_testdata("berlin.mbtiles");
	let temp_dir = TempDir::new().unwrap();
	let output = temp_dir.path().join("berlin.versatiles");

	// Convert using CLI (true E2E)
	versatiles_run(&format!("convert {} {}", input, output.to_str().unwrap()));

	// Read tiles from both source and output using library
	let runtime = TilesRuntime::builder().silent_progress(true).build();
	let source_reader = runtime.get_reader_from_str(&input).await.unwrap();
	let output_reader = runtime.get_reader_from_str(output.to_str().unwrap()).await.unwrap();

	// Test multiple tiles at different zoom levels
	let test_coords = [
		TileCoord::new(0, 0, 0).unwrap(),
		TileCoord::new(5, 17, 10).unwrap(),
		TileCoord::new(10, 550, 335).unwrap(),
		TileCoord::new(14, 8800, 5374).unwrap(),
	];

	for coord in test_coords {
		assert_tiles_equal(&source_reader, &output_reader, &coord).await;
	}
}

/// Test that tile data is preserved when converting mbtiles to pmtiles.
#[tokio::test]
async fn e2e_tile_integrity_mbtiles_to_pmtiles() {
	let input = get_testdata("berlin.mbtiles");
	let temp_dir = TempDir::new().unwrap();
	let output = temp_dir.path().join("berlin.pmtiles");

	// Convert using CLI
	versatiles_run(&format!("convert {} {}", input, output.to_str().unwrap()));

	// Verify tile integrity
	let runtime = TilesRuntime::builder().silent_progress(true).build();
	let source_reader = runtime.get_reader_from_str(&input).await.unwrap();
	let output_reader = runtime.get_reader_from_str(output.to_str().unwrap()).await.unwrap();

	// Test specific tile known to exist in berlin dataset
	let coord = TileCoord::new(14, 8800, 5374).unwrap();
	assert_tiles_equal(&source_reader, &output_reader, &coord).await;
}

/// Test that tile data is preserved through a chain of conversions.
#[tokio::test]
async fn e2e_tile_integrity_round_trip() {
	let input = get_testdata("berlin.mbtiles");
	let temp_dir = TempDir::new().unwrap();
	let versatiles_path = temp_dir.path().join("step1.versatiles");
	let pmtiles_path = temp_dir.path().join("step2.pmtiles");
	let final_path = temp_dir.path().join("step3.mbtiles");

	// Chain of conversions: mbtiles -> versatiles -> pmtiles -> mbtiles
	versatiles_run(&format!("convert {} {}", input, versatiles_path.to_str().unwrap()));
	versatiles_run(&format!(
		"convert {} {}",
		versatiles_path.to_str().unwrap(),
		pmtiles_path.to_str().unwrap()
	));
	versatiles_run(&format!(
		"convert {} {}",
		pmtiles_path.to_str().unwrap(),
		final_path.to_str().unwrap()
	));

	// Verify tile integrity end-to-end
	let runtime = TilesRuntime::builder().silent_progress(true).build();
	let source_reader = runtime.get_reader_from_str(&input).await.unwrap();
	let final_reader = runtime.get_reader_from_str(final_path.to_str().unwrap()).await.unwrap();

	// Test multiple tiles
	let test_coords = [
		TileCoord::new(0, 0, 0).unwrap(),
		TileCoord::new(10, 550, 335).unwrap(),
		TileCoord::new(14, 8800, 5374).unwrap(),
	];

	for coord in test_coords {
		assert_tiles_equal(&source_reader, &final_reader, &coord).await;
	}
}

/// Test that tiles with different compression settings have same decompressed content.
#[tokio::test]
async fn e2e_tile_integrity_with_recompression() {
	let input = get_testdata("berlin.mbtiles");
	let temp_dir = TempDir::new().unwrap();
	let output_br = temp_dir.path().join("berlin_brotli.versatiles");

	// Convert with Brotli compression
	versatiles_run(&format!(
		"convert --compress brotli {} {}",
		input,
		output_br.to_str().unwrap()
	));

	// Verify tile integrity (library handles decompression)
	let runtime = TilesRuntime::builder().silent_progress(true).build();
	let source_reader = runtime.get_reader_from_str(&input).await.unwrap();
	let output_reader = runtime.get_reader_from_str(output_br.to_str().unwrap()).await.unwrap();

	let coord = TileCoord::new(14, 8800, 5374).unwrap();
	assert_tiles_equal(&source_reader, &output_reader, &coord).await;
}

/// Test that bbox filtering preserves tile content for included tiles.
#[tokio::test]
async fn e2e_tile_integrity_with_bbox_filter() {
	let input = get_testdata("berlin.mbtiles");
	let temp_dir = TempDir::new().unwrap();
	let output = temp_dir.path().join("berlin_filtered.versatiles");

	// Convert with bbox filter (central Berlin area)
	versatiles_run(&format!(
		"convert --bbox 13.3,52.45,13.5,52.55 {} {}",
		input,
		output.to_str().unwrap()
	));

	// Verify tile content for a tile within the bbox
	let runtime = TilesRuntime::builder().silent_progress(true).build();
	let source_reader = runtime.get_reader_from_str(&input).await.unwrap();
	let output_reader = runtime.get_reader_from_str(output.to_str().unwrap()).await.unwrap();

	// This tile should be within the bbox
	let coord = TileCoord::new(14, 8802, 5373).unwrap();
	assert_tiles_equal(&source_reader, &output_reader, &coord).await;
}
