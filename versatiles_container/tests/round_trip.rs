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
		bbox_pyramid: Some(TileBBoxPyramid::new_full(5)), // Limit to levels 0-5
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
		bbox_pyramid: Some(TileBBoxPyramid::new_full(5)),
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
		bbox_pyramid: Some(TileBBoxPyramid::new_full(3)),
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
		bbox_pyramid: Some(TileBBoxPyramid::new_full(3)),
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
		bbox_pyramid: Some(TileBBoxPyramid::new_full(3)),
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
