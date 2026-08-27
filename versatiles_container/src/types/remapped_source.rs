//! A [`TileSource`] that relabels its coordinates.
//!
//! Wrapping a source to remap coordinates is a small idea with a sharp edge:
//! requests have to be mapped *backwards* and results *forwards*, consistently
//! across every way of asking for a tile. Writing that out per access path is
//! how `TilesConvertReader` came to answer `tile()` and `tile_stream()`
//! differently for the same coordinate (issue #230), so it is written once
//! here and shared.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use versatiles_core::{TileBBox, TileCoord, TileJSON, TilePyramid, TileRemap, TileStream};

use crate::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};

/// Presents a source under relabelled tile coordinates.
///
/// See [`TileRemap`] for what the relabellings are. The wrapper reports a
/// pyramid, metadata and TileJSON describing the *remapped* grid, so everything
/// downstream — writers, bounds, zoom ranges — sees the corrected view.
#[derive(Debug)]
pub struct RemappedTileSource {
	source: SharedTileSource,
	/// Applied to results coming out of the source.
	forward: TileRemap,
	/// Applied to requests going into it. Precomputed because it is needed on
	/// every call and is easy to forget.
	backward: TileRemap,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	pyramid: Arc<TilePyramid>,
}

impl RemappedTileSource {
	/// Wraps `source`, relabelling its coordinates by `remap`.
	///
	/// # Errors
	/// Returns an error if the source's pyramid cannot be read.
	pub async fn new(source: SharedTileSource, remap: TileRemap) -> Result<Self> {
		let mut pyramid = source.tile_pyramid().await?.as_ref().clone();
		remap.apply_to_pyramid(&mut pyramid);

		let mut metadata = source.metadata().clone();
		metadata.set_tile_pyramid(pyramid.clone());
		// Relabelling coordinates destroys any ordering the source promised: a
		// Hilbert-ordered source is not Hilbert-ordered once its tiles are
		// renamed.
		metadata.set_traversal(Traversal::ANY);

		let mut tilejson = source.tilejson().clone();
		metadata.update_tilejson(&mut tilejson);

		Ok(RemappedTileSource {
			source,
			forward: remap,
			backward: remap.inverted(),
			metadata,
			tilejson,
			pyramid: Arc::new(pyramid),
		})
	}

	/// The remap being applied.
	#[must_use]
	pub fn remap(&self) -> TileRemap {
		self.forward
	}
}

#[async_trait]
impl TileSource for RemappedTileSource {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("remap", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		Ok(self.pyramid.clone())
	}

	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		self.source.tile(&self.backward.remapped_coord(*coord)).await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let forward = self.forward;
		Ok(self
			.source
			.tile_stream(self.backward.remapped_bbox(bbox))
			.await?
			.map_coord(move |coord| forward.remapped_coord(coord)))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let forward = self.forward;
		Ok(self
			.source
			.tile_coord_stream(self.backward.remapped_bbox(bbox))
			.await?
			.map_coord(move |coord| forward.remapped_coord(coord)))
	}

	async fn tile_size_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, u32>> {
		let forward = self.forward;
		Ok(self
			.source
			.tile_size_stream(self.backward.remapped_bbox(bbox))
			.await?
			.map_coord(move |coord| forward.remapped_coord(coord)))
	}
}

#[cfg(test)]
mod tests {
	use versatiles_core::{TileCompression, TileFormat};

	use super::*;
	use crate::MockReader;

	fn all_remaps() -> Vec<TileRemap> {
		let mut out = Vec::new();
		for swap_xy in [false, true] {
			for flip_y in [false, true] {
				for flip_x in [false, true] {
					out.push(TileRemap::new(flip_x, flip_y, swap_xy));
				}
			}
		}
		out
	}

	async fn wrap(remap: TileRemap) -> Result<RemappedTileSource> {
		let source = MockReader::new_mock(
			TilePyramid::new_full_up_to(3),
			TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None),
		)?;
		RemappedTileSource::new(Arc::new(source), remap).await
	}

	/// The bug #230 records: `tile()` and `tile_stream()` disagreeing for the
	/// same coordinate. Checked for every remap and every access path, since a
	/// wrapper is only correct if all of them tell the same story.
	#[tokio::test]
	async fn every_access_path_agrees() -> Result<()> {
		for remap in all_remaps() {
			let wrapped = wrap(remap).await?;
			let bbox = TileBBox::new_full(3)?;

			let mut streamed = wrapped.tile_stream(bbox).await?.to_vec().await;
			streamed.sort_by_key(|(coord, _)| (coord.x, coord.y));

			assert!(!streamed.is_empty(), "{remap:?} produced nothing");

			for (coord, tile) in streamed {
				let via_tile = wrapped.tile(&coord).await?;
				assert!(via_tile.is_some(), "{remap:?}: tile() lost {coord:?}");

				let expected = tile.into_blob(&TileCompression::Gzip)?;
				let actual = via_tile.unwrap().into_blob(&TileCompression::Gzip)?;
				assert_eq!(expected, actual, "{remap:?}: tile() differs at {coord:?}");
			}

			// The coordinate-only and size streams must cover the same set.
			let mut coords = wrapped
				.tile_coord_stream(bbox)
				.await?
				.to_vec()
				.await
				.into_iter()
				.map(|(c, ())| (c.x, c.y))
				.collect::<Vec<_>>();
			let mut sizes = wrapped
				.tile_size_stream(bbox)
				.await?
				.to_vec()
				.await
				.into_iter()
				.map(|(c, _)| (c.x, c.y))
				.collect::<Vec<_>>();
			coords.sort_unstable();
			sizes.sort_unstable();
			assert_eq!(coords, sizes, "{remap:?}: coord and size streams disagree");
		}
		Ok(())
	}

	/// Remapping relabels tiles; it must not lose or invent any.
	#[tokio::test]
	async fn the_tile_count_is_preserved() -> Result<()> {
		let plain = wrap(TileRemap::identity()).await?;
		let expected = plain.tile_pyramid().await?.count_tiles();

		for remap in all_remaps() {
			let wrapped = wrap(remap).await?;
			assert_eq!(
				wrapped.tile_pyramid().await?.count_tiles(),
				expected,
				"{remap:?} changed the tile count"
			);
		}
		Ok(())
	}

	/// A relabelled source can no longer promise the order it was built with.
	#[tokio::test]
	async fn the_declared_traversal_is_reset() -> Result<()> {
		let wrapped = wrap(TileRemap {
			flip_y: true,
			..TileRemap::default()
		})
		.await?;
		assert!(wrapped.metadata().traversal().is_any());
		Ok(())
	}

	/// Applying a remap and then its inverse returns the original grid.
	#[tokio::test]
	async fn remapping_twice_round_trips() -> Result<()> {
		for remap in all_remaps() {
			let source = MockReader::new_mock(
				TilePyramid::new_full_up_to(3),
				TileSourceMetadata::new(TileFormat::MVT, TileCompression::Gzip, Traversal::ANY, None),
			)?;
			let once = RemappedTileSource::new(Arc::new(source), remap).await?;
			let twice = RemappedTileSource::new(Arc::new(once), remap.inverted()).await?;

			let coord = TileCoord::new(3, 1, 2)?;
			assert!(twice.tile(&coord).await?.is_some(), "{remap:?} did not round-trip");
		}
		Ok(())
	}
}
