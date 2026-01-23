//! Integration tests for round-trip conversions between container formats.
//!
//! These tests verify that tiles can be written to and read from various formats
//! without data loss or corruption.

use anyhow::Result;
use tempfile::TempDir;
use versatiles_container::*;
use versatiles_core::*;

#[tokio::test]
async fn read_mbtiles_source() -> Result<()> {
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read from existing testdata
	let reader = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	// Verify metadata is present
	assert_eq!(reader.metadata().tile_format, TileFormat::MVT);
	assert!(reader.metadata().bbox_pyramid.get_level_max().unwrap() > 0);

	// Verify tilejson is present
	let tilejson = reader.tilejson();
	assert!(!tilejson.as_string().is_empty());

	Ok(())
}

#[tokio::test]
async fn mbtiles_to_versatiles_round_trip() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("output.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let original_format = source.metadata().tile_format;

	// Write to versatiles
	runtime.write_to_path(source, &versatiles_path).await?;

	// Read back
	let reader = runtime.get_reader_from_str(versatiles_path.to_str().unwrap()).await?;
	assert_eq!(reader.metadata().tile_format, original_format);
	assert!(reader.metadata().bbox_pyramid.get_level_max().unwrap() > 0);

	Ok(())
}

#[tokio::test]
async fn mbtiles_to_tar_round_trip() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let tar_path = temp_dir.path().join("output.tar");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source (limit to small subset for faster test)
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(5)), // Limit to levels 0-5
		..Default::default()
	};
	let filtered = TilesConvertReader::new_from_reader(source, params)?;

	// Write to tar
	runtime.write_to_path(filtered.into_shared(), &tar_path).await?;

	// Read back
	let reader = runtime.get_reader_from_str(tar_path.to_str().unwrap()).await?;
	assert!(reader.metadata().bbox_pyramid.get_level_max().unwrap() > 0);

	Ok(())
}

#[tokio::test]
async fn mbtiles_to_pmtiles_round_trip() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let pmtiles_path = temp_dir.path().join("output.pmtiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source with filter
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(5)),
		..Default::default()
	};
	let filtered = TilesConvertReader::new_from_reader(source, params)?;

	// Write to pmtiles
	runtime.write_to_path(filtered.into_shared(), &pmtiles_path).await?;

	// Read back
	let reader = runtime.get_reader_from_str(pmtiles_path.to_str().unwrap()).await?;
	assert!(reader.metadata().bbox_pyramid.get_level_max().unwrap() > 0);

	Ok(())
}

#[tokio::test]
async fn converter_preserves_tile_format() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let output_path = temp_dir.path().join("converted.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let original_format = source.metadata().tile_format;
	let original_compression = source.metadata().tile_compression;

	// Convert without changing format/compression
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(3)),
		..Default::default()
	};

	convert_tiles_container(source, params, &output_path, runtime.clone()).await?;

	// Verify format is preserved
	let reader = runtime.get_reader_from_str(output_path.to_str().unwrap()).await?;
	assert_eq!(reader.metadata().tile_format, original_format);
	// Note: compression may change depending on writer implementation
	let _ = original_compression; // May differ based on writer

	Ok(())
}

#[tokio::test]
async fn converter_changes_compression() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let output_path = temp_dir.path().join("compressed.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	// Convert with Brotli compression
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(3)),
		tile_compression: Some(TileCompression::Brotli),
		..Default::default()
	};

	convert_tiles_container(source, params, &output_path, runtime.clone()).await?;

	// Verify output exists and is readable
	let reader = runtime.get_reader_from_str(output_path.to_str().unwrap()).await?;
	assert!(reader.metadata().bbox_pyramid.get_level_max().unwrap() > 0);

	Ok(())
}

#[tokio::test]
async fn individual_tile_access() -> Result<()> {
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	let reader = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	// Try to get a specific tile
	let coord = TileCoord::new(0, 0, 0)?;
	let tile = reader.get_tile(&coord).await?;

	// Level 0 should have a tile
	assert!(tile.is_some());

	Ok(())
}

#[tokio::test]
async fn metadata_consistency_after_conversion() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let output_path = temp_dir.path().join("meta_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read and convert
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(3)),
		..Default::default()
	};
	convert_tiles_container(source, params, &output_path, runtime.clone()).await?;

	// Read back and verify metadata
	let reader = runtime.get_reader_from_str(output_path.to_str().unwrap()).await?;
	let metadata = reader.metadata();

	// Basic metadata checks
	assert!(metadata.bbox_pyramid.get_level_min().is_some());
	assert!(metadata.bbox_pyramid.get_level_max().is_some());
	assert!(!metadata.tile_format.as_extension().is_empty());

	Ok(())
}

/// Verifies that actual tile DATA content is preserved after write→read cycle.
/// This is critical - metadata tests alone are not sufficient!
#[tokio::test]
async fn versatiles_tile_data_preserved_after_round_trip() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("tile_data_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	// Collect some tiles from the source BEFORE writing
	let test_coords = [
		TileCoord::new(0, 0, 0)?,
		TileCoord::new(4, 8, 5)?,
		TileCoord::new(6, 33, 21)?,
	];

	let mut original_tiles = Vec::new();
	for coord in &test_coords {
		if let Some(tile) = source.get_tile(coord).await? {
			let blob = tile.into_blob(TileCompression::Uncompressed)?;
			original_tiles.push((*coord, blob));
		}
	}

	assert!(!original_tiles.is_empty(), "Should have found at least one test tile");

	// Write to versatiles
	let source2 = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	runtime.write_to_path(source2, &versatiles_path).await?;

	// Read back from versatiles
	let reader = runtime.get_reader_from_str(versatiles_path.to_str().unwrap()).await?;

	// Verify each tile's content matches
	for (coord, original_blob) in &original_tiles {
		let read_tile = reader
			.get_tile(coord)
			.await?
			.unwrap_or_else(|| panic!("Tile at {coord:?} should exist after round-trip"));
		let read_blob = read_tile.into_blob(TileCompression::Uncompressed)?;

		assert_eq!(
			original_blob.as_slice(),
			read_blob.as_slice(),
			"Tile data at {coord:?} should be identical after round-trip"
		);
	}

	Ok(())
}

/// Verifies tile stream contains all expected tiles after write→read cycle.
#[tokio::test]
async fn versatiles_tile_stream_complete_after_round_trip() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("stream_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Read source and limit to a small bbox for faster testing
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let params = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(4)), // Levels 0-4
		..Default::default()
	};
	let filtered = TilesConvertReader::new_from_reader(source, params)?;

	// Count tiles in source stream
	let source_for_count = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;
	let params_for_count = TilesConverterParameters {
		bbox_pyramid: Some(TileBBoxPyramid::new_full_up_to(4)),
		..Default::default()
	};
	let filtered_for_count = TilesConvertReader::new_from_reader(source_for_count, params_for_count)?;
	let bbox = TileBBox::new_full(4)?;
	let source_tiles: Vec<_> = filtered_for_count
		.into_shared()
		.get_tile_stream(bbox)
		.await?
		.to_vec()
		.await;
	let source_tile_count = source_tiles.len();

	// Write to versatiles
	runtime.write_to_path(filtered.into_shared(), &versatiles_path).await?;

	// Read back and count tiles
	let reader = runtime.get_reader_from_str(versatiles_path.to_str().unwrap()).await?;
	let read_bbox = TileBBox::new_full(4)?;
	let read_tiles: Vec<_> = reader.get_tile_stream(read_bbox).await?.to_vec().await;

	assert_eq!(
		read_tiles.len(),
		source_tile_count,
		"Number of tiles should be preserved after round-trip"
	);

	Ok(())
}

/// Verifies tiles at block boundaries (255/256) are correctly handled.
#[tokio::test]
async fn versatiles_block_boundary_tiles_preserved() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("boundary_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Berlin mbtiles goes up to zoom 14, which has tiles up to 16383
	// At zoom 9, tiles go up to 511, so we can test boundary at 255/256
	let source = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	// Write to versatiles
	runtime.write_to_path(source, &versatiles_path).await?;

	// Read back
	let reader = runtime.get_reader_from_str(versatiles_path.to_str().unwrap()).await?;

	// Test tiles near block boundaries at zoom level 9 if they exist
	// Block 0 ends at 255, Block 1 starts at 256
	let boundary_coords = [
		TileCoord::new(9, 255, 170)?, // Last column in block 0
		TileCoord::new(9, 256, 170)?, // First column in block 1
		TileCoord::new(9, 270, 255)?, // Last row in block row 0
		TileCoord::new(9, 270, 256)?, // First row in block row 1
	];

	let source2 = runtime.get_reader_from_str("../testdata/berlin.mbtiles").await?;

	for coord in &boundary_coords {
		let original = source2.get_tile(coord).await?;
		let read = reader.get_tile(coord).await?;

		match (original, read) {
			(Some(orig_tile), Some(read_tile)) => {
				let orig_blob = orig_tile.into_blob(TileCompression::Uncompressed)?;
				let read_blob = read_tile.into_blob(TileCompression::Uncompressed)?;
				assert_eq!(
					orig_blob.as_slice(),
					read_blob.as_slice(),
					"Boundary tile at {coord:?} should have identical content"
				);
			}
			(None, None) => {
				// Both missing is OK - tile may not exist in Berlin data
			}
			(Some(_), None) => {
				panic!("Tile at {coord:?} exists in source but missing after round-trip");
			}
			(None, Some(_)) => {
				panic!("Tile at {coord:?} missing in source but present after round-trip");
			}
		}
	}

	Ok(())
}

/// Verifies that tiles written with deduplication can still be read correctly.
#[tokio::test]
async fn versatiles_deduplicated_tiles_readable() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("dedup_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Create a mock source with duplicate small tiles
	let mut source = MockReader::new_mock(TileSourceMetadata::new(
		TileFormat::MVT,
		TileCompression::Uncompressed,
		TileBBoxPyramid::new_full_up_to(2), // Small: 4x4 = 16 tiles at level 2
		Traversal::ANY,
	))?;

	// Get the original tiles before writing
	let bbox = TileBBox::new_full(2)?;
	let original_tiles: Vec<(TileCoord, Blob)> = source
		.get_tile_stream(bbox)
		.await?
		.map_parallel_try(|_coord, tile: Tile| tile.into_blob(TileCompression::Uncompressed))
		.unwrap_results()
		.to_vec()
		.await;

	// Write to versatiles (MockReader tiles are small and may be deduplicated)
	VersaTilesWriter::write_to_path(&mut source, &versatiles_path, runtime.clone()).await?;

	// Read back
	let reader = runtime.get_reader_from_str(versatiles_path.to_str().unwrap()).await?;

	// Verify each tile can be read and matches original
	for (coord, original_blob) in &original_tiles {
		let read_tile = reader
			.get_tile(coord)
			.await?
			.unwrap_or_else(|| panic!("Tile at {coord:?} should exist"));
		let read_blob = read_tile.into_blob(TileCompression::Uncompressed)?;

		assert_eq!(
			original_blob.as_slice(),
			read_blob.as_slice(),
			"Deduplicated tile at {coord:?} should be readable with correct content"
		);
	}

	Ok(())
}

/// Verifies that empty tile sources (with no zoom levels) produce an error.
/// The VersaTiles format requires a valid zoom range, so an empty bbox_pyramid cannot be written.
#[tokio::test]
async fn versatiles_empty_source_fails_gracefully() -> Result<()> {
	let temp_dir = TempDir::new()?;
	let versatiles_path = temp_dir.path().join("empty_test.versatiles");
	let runtime = TilesRuntime::builder().silent_progress(true).build();

	// Create an empty mock source (no zoom levels at all)
	let mut source = MockReader::new_mock(TileSourceMetadata::new(
		TileFormat::MVT,
		TileCompression::Uncompressed,
		TileBBoxPyramid::new_empty(),
		Traversal::ANY,
	))?;

	// Writing should fail because the VersaTiles format requires valid minzoom/maxzoom
	let result = VersaTilesWriter::write_to_path(&mut source, &versatiles_path, runtime.clone()).await;
	assert!(
		result.is_err(),
		"Writing an empty source should fail because VersaTiles requires valid zoom range"
	);

	let err = result.unwrap_err();
	let err_chain = format!("{err:?}");
	assert!(
		err_chain.contains("minzoom") || err_chain.contains("maxzoom"),
		"Error should mention zoom range issue: {err_chain}"
	);

	Ok(())
}
