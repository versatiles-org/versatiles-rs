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
//!         Ok(stream.map_item_parallel(|tile| /* transform tile */ Ok(tile)))
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
/// - Cloned metadata (parameters, TileJSON, traversal) from the source
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
	/// Clones metadata (parameters, TileJSON, traversal) from the source, which can
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
	pub fn with_parameters(mut self, parameters: TileSourceMetadata) -> Self {
		self.parameters = parameters;
		self
	}

	/// Builder method to override the TileJSON metadata.
	///
	/// Use this when the processor modifies metadata (e.g., updating attribution, bounds).
	pub fn with_tilejson(mut self, tilejson: TileJSON) -> Self {
		self.tilejson = tilejson;
		self
	}

	/// Builder method to override the traversal hint.
	///
	/// Use this when the processor has a preferred read order different from its source.
	pub fn with_traversal(mut self, traversal: Traversal) -> Self {
		self.traversal = traversal;
		self
	}

	/// Returns the processor's name.
	pub fn name(&self) -> &str {
		&self.name
	}

	/// Returns the upstream source as a trait object reference.
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
	pub fn parameters(&self) -> &TileSourceMetadata {
		&self.parameters
	}

	/// Returns the (potentially modified) TileJSON.
	pub fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	/// Returns the (potentially modified) traversal hint.
	pub fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	/// Returns a mutable reference to the parameters.
	///
	/// Use this to modify parameters after construction.
	pub fn parameters_mut(&mut self) -> &mut TileSourceMetadata {
		&mut self.parameters
	}

	/// Returns a mutable reference to the TileJSON.
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
