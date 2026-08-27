//! This module defines metadata describing tile source output characteristics.

use std::sync::{Arc, RwLock};

use anyhow::Result;
use versatiles_core::{TileBBox, TileCompression, TileFormat, TileJSON, TilePyramid, TileSchema, TileType};

use crate::Traversal;

/// Metadata describing the output characteristics of a tile source.
///
/// # Fields
/// - `tile_pyramid`: The bounding box and zoom pyramid defining the tile coverage.
/// - `tile_compression`: The compression algorithm applied to tiles (e.g., gzip, brotli).
/// - `tile_format`: The format of the tiles (e.g., PNG, JPEG, MVT).
#[derive(Debug, Default)]
pub struct TileSourceMetadata {
	/// The compression algorithm applied to tiles (e.g., gzip, brotli).
	tile_compression: TileCompression,
	/// The format of the tiles (e.g., PNG, JPEG, MVT).
	tile_format: TileFormat,

	traversal: Traversal,

	tile_pyramid: Arc<RwLock<Option<Arc<TilePyramid>>>>,
}

impl Clone for TileSourceMetadata {
	/// Produces an independent copy whose `tile_pyramid` lock is *not* shared
	/// with `self`. The inner `Arc<TilePyramid>` itself is cheaply ref-bumped.
	///
	/// Sharing the lock (as a naive derived `Clone` would) is unsound here:
	/// `set_tile_pyramid` would mutate every sibling clone, silently corrupting
	/// every source that has been wrapped by a converter/filter/operation.
	fn clone(&self) -> Self {
		let pyramid = self.tile_pyramid.read().expect("poisoned RwLock").clone();
		TileSourceMetadata {
			tile_compression: self.tile_compression,
			tile_format: self.tile_format,
			traversal: self.traversal.clone(),
			tile_pyramid: Arc::new(RwLock::new(pyramid)),
		}
	}
}

impl PartialEq for TileSourceMetadata {
	fn eq(&self, other: &Self) -> bool {
		self.tile_compression == other.tile_compression
			&& self.tile_format == other.tile_format
			&& self.traversal == other.traversal
	}
}

impl TileSourceMetadata {
	/// Create a new `TileSourceMetadata`.
	///
	/// # Arguments
	/// * `tile_format` - The format of the tiles.
	/// * `tile_compression` - The compression algorithm applied to tiles.
	/// * `tile_pyramid` - The bounding box and zoom pyramid defining the tile coverage.
	///
	/// # Returns
	/// A new instance of `TileSourceMetadata` configured with the specified parameters.
	#[must_use]
	pub fn new(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		traversal: Traversal,
		tile_pyramid: Option<TilePyramid>,
	) -> TileSourceMetadata {
		TileSourceMetadata {
			tile_compression,
			tile_format,
			traversal,
			tile_pyramid: Arc::new(RwLock::new(tile_pyramid.map(Arc::new))),
		}
	}

	/// Updates fields using information from [`TileSourceMetadata`].
	///
	/// - Applies [`TileJSON::update_from_pyramid`] to intersect/set bounds and min/max zoom.
	/// - Sets `tile_format` from the reader parameters and derives `tile_type` from it.
	/// - If `tile_schema` is absent or mismatched with `tile_type`, infers a suitable schema
	///   (e.g., `RasterRGB` for rasters; for vectors, derived from `vector_layers`).
	pub fn update_tilejson(&self, tile_json: &mut TileJSON) {
		if let Some(tile_pyramid) = self.tile_pyramid() {
			tile_json.update_from_pyramid(tile_pyramid.as_ref());
		}

		tile_json.tile_format = Some(self.tile_format);

		tile_json.tile_type = tile_json.tile_format.map(|f| f.to_type());

		if let Some(tile_type) = tile_json.tile_type
			&& tile_json.tile_schema.map(|s| s.tile_type()) != tile_json.tile_type
		{
			tile_json.tile_schema = Some(match tile_type {
				TileType::Raster => TileSchema::RasterRGB,
				TileType::Vector => tile_json.vector_layers.tile_schema(),
				TileType::Unknown => TileSchema::Unknown,
			});
		}
	}

	/// The order and granularity in which this source hands out tiles.
	#[must_use]
	pub fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	/// The compression the source's tiles are stored with.
	#[must_use]
	pub fn tile_compression(&self) -> &TileCompression {
		&self.tile_compression
	}

	/// The format the source's tiles are stored in.
	#[must_use]
	pub fn tile_format(&self) -> &TileFormat {
		&self.tile_format
	}

	/// Replaces the traversal, for an operation that changes the order or
	/// granularity of the tiles it passes on.
	pub fn set_traversal(&mut self, traversal: Traversal) {
		self.traversal = traversal;
	}

	/// Records that tiles are now stored with a different compression.
	pub fn set_tile_compression(&mut self, tile_compression: TileCompression) {
		self.tile_compression = tile_compression;
	}

	/// Records that tiles are now stored in a different format.
	pub fn set_tile_format(&mut self, tile_format: TileFormat) {
		self.tile_format = tile_format;
	}

	/// The source's tile pyramid, if one has been computed or set.
	///
	/// `None` means nobody has asked yet — not that the source is empty. Use
	/// [`get_or_compute_tile_pyramid`](Self::get_or_compute_tile_pyramid) to
	/// compute it on first use.
	#[must_use]
	pub fn tile_pyramid(&self) -> Option<Arc<TilePyramid>> {
		self.tile_pyramid.read().expect("poisoned RwLock").clone()
	}

	/// Overwrites the tile pyramid. Requires `&mut self` so that mutations cannot
	/// accidentally propagate through a shared clone — see the `Clone` impl above
	/// for the rationale.
	pub fn set_tile_pyramid(&mut self, tile_pyramid: TilePyramid) {
		*self.tile_pyramid.write().expect("poisoned RwLock") = Some(Arc::new(tile_pyramid));
	}

	/// Returns the tile pyramid, computing it with `compute_fn` if it is not
	/// there yet.
	///
	/// `compute_fn` runs while **no** lock is held. That matters because it
	/// scans the whole container — for `MBTilesReader` it is a full
	/// `SELECT … FROM tiles` — and every reader of this metadata, including
	/// [`tile_pyramid`](Self::tile_pyramid), would otherwise block for the
	/// length of that scan.
	///
	/// The cost is that two threads racing on a cold pyramid can both run
	/// `compute_fn`. They compute the same pyramid, so the only loss is the
	/// duplicated work; the first result to be stored is the one every caller
	/// gets, so they all end up sharing one `Arc`.
	pub fn get_or_compute_tile_pyramid(
		&self,
		compute_fn: impl FnOnce() -> Result<TilePyramid>,
	) -> Result<Arc<TilePyramid>> {
		if let Some(pyramid) = self.tile_pyramid() {
			return Ok(pyramid);
		}

		let computed = Arc::new(compute_fn()?);

		// Another thread may have finished computing while this one was
		// scanning; keep whichever result landed first.
		let mut write_guard = self.tile_pyramid.write().expect("poisoned RwLock");
		Ok(write_guard.get_or_insert(computed).clone())
	}

	/// Narrows `bbox` to what this source actually holds.
	///
	/// Returns `bbox` unchanged when no pyramid has been computed, so a caller
	/// never loses tiles by asking early — it just does not gain the narrowing.
	#[must_use]
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> TileBBox {
		if let Some(tile_pyramid) = self.tile_pyramid() {
			bbox.intersection_pyramid(&tile_pyramid)
		} else {
			*bbox
		}
	}
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use versatiles_core::GeoBBox;

	use super::*;

	/// `compute_fn` must run without the write lock held, or every reader of
	/// this metadata stalls for the length of a full container scan.
	///
	/// Against the previous implementation, which took the write lock before
	/// calling `compute_fn`, the reader below blocks and this test fails on the
	/// `recv_timeout`.
	#[test]
	fn readers_are_not_blocked_while_the_pyramid_is_computed() {
		use std::{sync::mpsc, thread, time::Duration};

		let metadata = Arc::new(TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			Traversal::ANY,
			None,
		));

		let (inside_tx, inside_rx) = mpsc::channel();
		let (release_tx, release_rx) = mpsc::channel();

		let writer = {
			let metadata = Arc::clone(&metadata);
			thread::spawn(move || {
				metadata.get_or_compute_tile_pyramid(|| {
					inside_tx.send(()).expect("test receiver alive");
					release_rx.recv().expect("test sender alive");
					Ok(TilePyramid::new_empty())
				})
			})
		};

		// Wait until the writer is provably inside `compute_fn`.
		inside_rx.recv().expect("writer reached compute_fn");

		let (read_tx, read_rx) = mpsc::channel();
		{
			let metadata = Arc::clone(&metadata);
			thread::spawn(move || {
				let _ = read_tx.send(metadata.tile_pyramid().is_none());
			});
		}

		let still_cold = read_rx
			.recv_timeout(Duration::from_secs(10))
			.expect("a reader blocked while the pyramid was being computed");
		assert!(
			still_cold,
			"the pyramid should not be published before compute_fn returns"
		);

		release_tx.send(()).expect("writer alive");
		writer.join().expect("writer did not panic").expect("compute succeeded");
		assert!(metadata.tile_pyramid().is_some(), "the pyramid is published afterwards");
	}

	#[test]
	fn test_tiles_reader_parameters_new() {
		let tile_pyramid = TilePyramid::from_geo_bbox(0, 10, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap()).unwrap();
		let tile_format = TileFormat::PNG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new(
			tile_format,
			tile_compression,
			Traversal::ANY,
			Some(tile_pyramid.clone()),
		);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.tile_pyramid().unwrap().as_ref(), &tile_pyramid);
	}

	#[test]
	fn test_tiles_reader_parameters_new_full() {
		let tile_format = TileFormat::JPG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new(tile_format, tile_compression, Traversal::ANY, None);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert!(params.tile_pyramid().is_none());
	}

	#[test]
	fn should_update_tile_json() -> Result<()> {
		let mut tj = TileJSON::default();
		// Prepare reader parameters
		let tile_pyramid = TilePyramid::from_geo_bbox(1, 4, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap())?;
		let rp = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(tile_pyramid),
		);
		rp.update_tilejson(&mut tj);
		// Bounds and zooms
		assert_eq!(tj.zoom_min(), Some(1));
		assert_eq!(tj.zoom_max(), Some(4));
		// Format, content, and schema
		assert_eq!(tj.tile_format, Some(TileFormat::PNG));
		assert_eq!(tj.tile_type, Some(TileType::Raster));
		assert_eq!(tj.tile_schema, Some(TileSchema::RasterRGB));
		Ok(())
	}
}
