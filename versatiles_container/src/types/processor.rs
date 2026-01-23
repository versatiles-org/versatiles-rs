//! Base pattern for implementing tile processors.
//!
//! This module provides [`TileProcessor`], a base struct that simplifies implementing
//! tile processors (sources that wrap and transform upstream tile sources).
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use versatiles_container::{TileProcessor, TileSource, SourceType};
//!
//! struct MyProcessor {
//!     base: TileProcessor,
//!     // processor-specific fields
//! }
//!
//! impl TileSource for MyProcessor {
//!     fn source_name(&self) -> &str {
//!         self.base.name()
//!     }
//!
//!     fn source_type(&self) -> SourceType {
//!         SourceType::Processor("my_processor")
//!     }
//!
//!     fn parameters(&self) -> &TileSourceMetadata {
//!         self.base.parameters()
//!     }
//!
//!     fn tilejson(&self) -> &TileJSON {
//!         self.base.tilejson()
//!     }
//!
//!     fn traversal(&self) -> &Traversal {
//!         self.base.traversal()
//!     }
//!
//!     async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
//!         // Apply transformation to upstream stream
//!         let stream = self.base.source().get_tile_stream(bbox).await?;
//!         Ok(stream.map_parallel_try(|_coord, tile| /* transform tile */ Ok(tile)))
//!     }
//!
//!     // ... other methods
//! }
//! ```

use crate::{TileSource, TileSourceMetadata, Traversal};
use versatiles_core::TileJSON;

/// Base struct for tile processors that wrap a single upstream source.
///
/// This struct provides:
/// - Storage for the upstream source
/// - Cloned metadata (parameters, `TileJSON`, traversal) from the source
/// - Builder pattern for modifying metadata
/// - Accessor methods for implementation convenience
///
/// Processors should embed this struct and delegate trait method implementations
/// to its methods where appropriate.
#[derive(Debug)]
pub struct TileProcessor {
	name: String,
	source: Box<dyn TileSource>,
	parameters: TileSourceMetadata,
	tilejson: TileJSON,
	traversal: Traversal,
}

impl TileProcessor {
	/// Creates a new processor wrapping the given source.
	///
	/// Clones metadata (parameters, `TileJSON`, traversal) from the source, which can
	/// then be modified via builder methods.
	///
	/// # Arguments
	///
	/// * `name` - Human-readable name for this processor (e.g., "filter", "converter")
	/// * `source` - The upstream tile source to wrap
	pub fn new(name: impl Into<String>, source: Box<dyn TileSource>) -> Self {
		let parameters = source.metadata().clone();
		let tilejson = source.tilejson().clone();
		let traversal = parameters.traversal.clone();

		Self {
			name: name.into(),
			source,
			parameters,
			tilejson,
			traversal,
		}
	}

	/// Builder method to override the parameters.
	///
	/// Use this when the processor modifies spatial extent, compression, or format.
	#[must_use]
	pub fn with_parameters(mut self, parameters: TileSourceMetadata) -> Self {
		self.parameters = parameters;
		self
	}

	/// Builder method to override the `TileJSON` metadata.
	///
	/// Use this when the processor modifies metadata (e.g., updating attribution, bounds).
	#[must_use]
	pub fn with_tilejson(mut self, tilejson: TileJSON) -> Self {
		self.tilejson = tilejson;
		self
	}

	/// Builder method to override the traversal hint.
	///
	/// Use this when the processor has a preferred read order different from its source.
	#[must_use]
	pub fn with_traversal(mut self, traversal: Traversal) -> Self {
		self.traversal = traversal;
		self
	}

	/// Returns the processor's name.
	#[must_use]
	pub fn name(&self) -> &str {
		&self.name
	}

	/// Returns the upstream source as a trait object reference.
	#[must_use]
	pub fn source(&self) -> &dyn TileSource {
		&*self.source
	}

	/// Returns a mutable reference to the boxed source.
	///
	/// Use this when you need to call mutable methods on the source (e.g., `override_compression`).
	pub fn source_mut(&mut self) -> &mut Box<dyn TileSource> {
		&mut self.source
	}

	/// Returns the (potentially modified) parameters.
	#[must_use]
	pub fn parameters(&self) -> &TileSourceMetadata {
		&self.parameters
	}

	/// Returns the (potentially modified) `TileJSON`.
	#[must_use]
	pub fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the (potentially modified) traversal hint.
	#[must_use]
	pub fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	/// Returns a mutable reference to the parameters.
	///
	/// Use this to modify parameters after construction.
	pub fn parameters_mut(&mut self) -> &mut TileSourceMetadata {
		&mut self.parameters
	}

	/// Returns a mutable reference to the `TileJSON`.
	///
	/// Use this to modify metadata after construction.
	pub fn tilejson_mut(&mut self) -> &mut TileJSON {
		&mut self.tilejson
	}

	/// Returns a mutable reference to the traversal hint.
	///
	/// Use this to modify traversal after construction.
	pub fn traversal_mut(&mut self) -> &mut Traversal {
		&mut self.traversal
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{MockReader, Traversal};
	use versatiles_core::{TileBBoxPyramid, TileCompression, TileFormat};

	fn create_mock_source() -> Box<dyn TileSource> {
		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full_up_to(10),
			Traversal::new_any(),
		);
		MockReader::new_mock(metadata).unwrap().boxed()
	}

	#[test]
	fn test_new_processor() {
		let source = create_mock_source();
		let processor = TileProcessor::new("test_processor", source);

		assert_eq!(processor.name(), "test_processor");
		assert!(processor.parameters().tile_format == TileFormat::PNG);
		assert!(processor.parameters().tile_compression == TileCompression::Uncompressed);
	}

	#[test]
	fn test_new_processor_clones_metadata() {
		let source = create_mock_source();
		let original_metadata = source.metadata().clone();

		let processor = TileProcessor::new("test", source);

		// Verify metadata was cloned
		assert_eq!(processor.parameters().tile_format, original_metadata.tile_format);
		assert_eq!(
			processor.parameters().tile_compression,
			original_metadata.tile_compression
		);
		assert_eq!(processor.parameters().bbox_pyramid, original_metadata.bbox_pyramid);
	}

	#[test]
	fn test_name_into_string() {
		let source = create_mock_source();
		let processor = TileProcessor::new("my_processor".to_string(), source);

		assert_eq!(processor.name(), "my_processor");
	}

	#[test]
	fn test_name_from_str() {
		let source = create_mock_source();
		let processor = TileProcessor::new("my_processor", source);

		assert_eq!(processor.name(), "my_processor");
	}

	#[test]
	fn test_source_accessor() {
		let source = create_mock_source();
		let processor = TileProcessor::new("test", source);

		// Verify source is accessible
		let source_ref = processor.source();
		assert_eq!(source_ref.metadata().tile_format, TileFormat::PNG);
	}

	#[test]
	fn test_source_mut_accessor() {
		let source = create_mock_source();
		let mut processor = TileProcessor::new("test", source);

		// Verify mutable source is accessible
		let source_mut = processor.source_mut();
		assert_eq!(source_mut.metadata().tile_format, TileFormat::PNG);
	}

	#[test]
	fn test_parameters_accessor() {
		let source = create_mock_source();
		let processor = TileProcessor::new("test", source);

		let params = processor.parameters();
		assert_eq!(params.tile_format, TileFormat::PNG);
		assert_eq!(params.tile_compression, TileCompression::Uncompressed);
	}

	#[test]
	fn test_tilejson_accessor() {
		let source = create_mock_source();
		let processor = TileProcessor::new("test", source);

		let tilejson = processor.tilejson();
		// TileJSON should be valid
		assert!(tilejson.as_string().contains("tilejson"));
	}

	#[test]
	fn test_traversal_accessor() {
		let source = create_mock_source();
		let processor = TileProcessor::new("test", source);

		let traversal = processor.traversal();
		// Should have the default traversal
		assert!(traversal.is_any());
	}

	#[test]
	fn test_with_parameters_builder() {
		let source = create_mock_source();
		let new_metadata = TileSourceMetadata::new(
			TileFormat::JPG,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full_up_to(5),
			Traversal::new_any(),
		);

		let processor = TileProcessor::new("test", source).with_parameters(new_metadata.clone());

		// Verify parameters were replaced
		assert_eq!(processor.parameters().tile_format, TileFormat::JPG);
		assert_eq!(processor.parameters().tile_compression, TileCompression::Gzip);
	}

	#[test]
	fn test_with_tilejson_builder() {
		let source = create_mock_source();
		let new_tilejson = TileJSON::default();

		let processor = TileProcessor::new("test", source).with_tilejson(new_tilejson.clone());

		// Verify TileJSON was replaced (default TileJSON should contain "3.0.0")
		assert!(processor.tilejson().as_string().contains("3.0.0"));
	}

	#[test]
	fn test_with_traversal_builder() {
		let source = create_mock_source();
		let new_traversal = Traversal::new_any_size(256, 256).unwrap();

		let processor = TileProcessor::new("test", source).with_traversal(new_traversal.clone());

		// Verify traversal was replaced
		assert_eq!(processor.traversal().max_size().unwrap(), 256);
	}

	#[test]
	fn test_builder_chaining() {
		let source = create_mock_source();
		let new_metadata = TileSourceMetadata::new(
			TileFormat::WEBP,
			TileCompression::Brotli,
			TileBBoxPyramid::new_full_up_to(8),
			Traversal::new_any(),
		);
		let new_tilejson = TileJSON::default();
		let new_traversal = Traversal::new_any_size(128, 128).unwrap();

		let processor = TileProcessor::new("test", source)
			.with_parameters(new_metadata)
			.with_tilejson(new_tilejson)
			.with_traversal(new_traversal);

		// Verify all were set
		assert_eq!(processor.parameters().tile_format, TileFormat::WEBP);
		assert_eq!(processor.parameters().tile_compression, TileCompression::Brotli);
		assert!(processor.tilejson().as_string().contains("3.0.0"));
		assert_eq!(processor.traversal().max_size().unwrap(), 128);
	}

	#[test]
	fn test_parameters_mut() {
		let source = create_mock_source();
		let mut processor = TileProcessor::new("test", source);

		// Modify parameters via mutable reference
		let params_mut = processor.parameters_mut();
		params_mut.tile_format = TileFormat::MVT;

		// Verify modification
		assert_eq!(processor.parameters().tile_format, TileFormat::MVT);
	}

	#[test]
	fn test_tilejson_mut() {
		let source = create_mock_source();
		let mut processor = TileProcessor::new("test", source);

		// Replace TileJSON via mutable reference
		*processor.tilejson_mut() = TileJSON::default();

		// Verify modification (should still be valid TileJSON)
		assert!(processor.tilejson().as_string().contains("3.0.0"));
	}

	#[test]
	fn test_traversal_mut() {
		let source = create_mock_source();
		let mut processor = TileProcessor::new("test", source);

		// Replace traversal via mutable reference
		*processor.traversal_mut() = Traversal::new_any_size(512, 512).unwrap();

		// Verify modification
		assert_eq!(processor.traversal().max_size().unwrap(), 512);
	}

	#[test]
	fn test_processor_preserves_source() {
		let source = create_mock_source();
		let source_metadata = source.metadata().clone();

		let processor = TileProcessor::new("test", source);

		// Verify source metadata is still accessible through processor
		assert_eq!(processor.source().metadata().tile_format, source_metadata.tile_format);
		assert_eq!(processor.source().metadata().bbox_pyramid, source_metadata.bbox_pyramid);
	}

	#[test]
	fn test_multiple_processors() {
		let source1 = create_mock_source();
		let source2 = create_mock_source();

		let processor1 = TileProcessor::new("processor1", source1);
		let processor2 = TileProcessor::new("processor2", source2);

		// Both should work independently
		assert_eq!(processor1.name(), "processor1");
		assert_eq!(processor2.name(), "processor2");
	}

	#[test]
	fn test_processor_with_different_formats() {
		// Test with MVT format
		let metadata_mvt = TileSourceMetadata::new(
			TileFormat::MVT,
			TileCompression::Gzip,
			TileBBoxPyramid::new_full_up_to(14),
			Traversal::new_any(),
		);
		let source_mvt = MockReader::new_mock(metadata_mvt).unwrap().boxed();
		let processor_mvt = TileProcessor::new("mvt_processor", source_mvt);

		assert_eq!(processor_mvt.parameters().tile_format, TileFormat::MVT);
		assert_eq!(processor_mvt.parameters().tile_compression, TileCompression::Gzip);
	}

	#[test]
	fn test_processor_with_different_compressions() {
		// Test with Brotli compression
		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Brotli,
			TileBBoxPyramid::new_full_up_to(10),
			Traversal::new_any(),
		);
		let source = MockReader::new_mock(metadata).unwrap().boxed();
		let processor = TileProcessor::new("brotli_processor", source);

		assert_eq!(processor.parameters().tile_compression, TileCompression::Brotli);
	}

	#[test]
	fn test_processor_with_custom_bbox() {
		// Create a pyramid with specific bbox
		let pyramid = TileBBoxPyramid::new_full_up_to(5);

		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			pyramid.clone(),
			Traversal::new_any(),
		);
		let source = MockReader::new_mock(metadata).unwrap().boxed();
		let processor = TileProcessor::new("bbox_processor", source);

		assert_eq!(processor.parameters().bbox_pyramid, pyramid);
	}

	#[test]
	fn test_processor_with_custom_traversal() {
		let traversal = Traversal::new_any_size(64, 64).unwrap();
		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full_up_to(10),
			traversal.clone(),
		);
		let source = MockReader::new_mock(metadata).unwrap().boxed();
		let processor = TileProcessor::new("traversal_processor", source);

		assert_eq!(processor.traversal().max_size().unwrap(), 64);
	}

	#[test]
	fn test_debug_impl() {
		let source = create_mock_source();
		let processor = TileProcessor::new("debug_test", source);

		// Should be debuggable
		let debug_str = format!("{processor:?}");
		assert!(debug_str.contains("TileProcessor"));
	}

	#[test]
	fn test_empty_name() {
		let source = create_mock_source();
		let processor = TileProcessor::new("", source);

		assert_eq!(processor.name(), "");
	}

	#[test]
	fn test_long_name() {
		let source = create_mock_source();
		let long_name = "a".repeat(1000);
		let processor = TileProcessor::new(long_name.clone(), source);

		assert_eq!(processor.name(), long_name);
	}

	#[test]
	fn test_unicode_name() {
		let source = create_mock_source();
		let processor = TileProcessor::new("日本語 🦀 Ελληνικά", source);

		assert_eq!(processor.name(), "日本語 🦀 Ελληνικά");
	}
}
