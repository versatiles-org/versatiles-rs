//! Sources that load vector features from on-disk formats.
//!
//! Each format adapter implements [`FeatureSource`] and yields features as a
//! stream. v1 adapters fully read the input synchronously and emit from
//! [`futures::stream::iter`]; future streaming adapters (FlatGeobuf, OSM PBF)
//! can pull records on demand without changing the public API.

mod csv;
mod geojson;
mod shapefile;

pub use csv::{CsvSource, CsvSourceBuilder};
pub use geojson::{Format as GeoJsonFormat, GeoJsonSource};
pub use shapefile::ShapefileSource;

use crate::geo::GeoFeature;
use anyhow::Result;
use futures::stream::BoxStream;

/// A source that produces a stream of [`GeoFeature`]s loaded from an on-disk format.
pub trait FeatureSource: Send {
	/// Load all features as a stream.
	///
	/// v1 implementations may fully read the input first and emit the resulting
	/// features via [`futures::stream::iter`]; later implementations are free to
	/// stream records on demand. Either way the caller drains the stream to get
	/// every feature.
	///
	/// **v1 limitation**: this method is synchronous. A genuinely streaming
	/// source that needs to perform async I/O during setup (FlatGeobuf with HTTP
	/// range requests, OSM PBF block index over the network) will require
	/// changing this to `async fn`. We're deferring that breaking change until
	/// such a source is added; existing v1 adapters do their I/O synchronously
	/// inside this method.
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>>;

	/// Short, human-readable name for this source — typically the filename stem.
	/// Used as the default MVT layer name.
	fn name(&self) -> &str;
}
