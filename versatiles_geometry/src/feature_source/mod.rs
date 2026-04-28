//! Sources that load vector features from on-disk formats.
//!
//! Each format adapter implements [`FeatureSource`] and yields features as a
//! stream. v1 adapters fully read the input synchronously and emit from
//! [`futures::stream::iter`]; future streaming adapters (FlatGeobuf, OSM PBF)
//! can pull records on demand without changing the public API.

mod geojson;

pub use geojson::GeoJsonSource;

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
	fn load(&self) -> Result<BoxStream<'static, Result<GeoFeature>>>;

	/// Short, human-readable name for this source — typically the filename stem.
	/// Used as the default MVT layer name.
	fn name(&self) -> &str;
}
