use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{collections::HashSet, fmt::Debug, sync::Arc};
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{GeoBBox, TileBBox, TileJSON, TileStream};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box, zoom levels, and/or the tile coordinates present in another container.
struct Args {
	/// Bounding box in WGS84: [min lng, min lat, max lng, max lat].
	bbox: Option<[f64; 4]>,
	/// minimal zoom level
	level_min: Option<u8>,
	/// maximal zoom level
	level_max: Option<u8>,
	/// Path to a tile container used as a coordinate allow-list.
	/// Only tiles whose coordinates exist in this container are passed through.
	/// Accepts the same path/URL syntax as `from_container`.
	/// Note: opening the container and building the allow-list requires I/O at pipeline build time.
	filename: Option<String>,
}

#[derive(Debug)]
struct Operation {
	metadata: TileSourceMetadata,
	source: Box<dyn TileSource>,
	mask: Option<Box<dyn TileSource>>,
	tilejson: TileJSON,
}

impl Operation {
	#[context("Building filter operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let mut metadata = source.metadata().clone();
		let mut tilejson = source.tilejson().clone();

		if let (Some(lo), Some(hi)) = (args.level_min, args.level_max)
			&& lo > hi
		{
			bail!(
				"Invalid zoom range in filter node {:?}: level_min ({lo}) must be ≤ level_max ({hi})",
				vpl_node.name
			);
		}

		if let Some(mut level_min) = args.level_min {
			if let Some(existing_level_min) = metadata.bbox_pyramid.level_min() {
				level_min = level_min.max(existing_level_min);
			}
			metadata.bbox_pyramid.set_level_min(level_min);
			tilejson.set_min_zoom(level_min);
		}

		if let Some(mut level_max) = args.level_max {
			if let Some(existing_level_max) = metadata.bbox_pyramid.level_max() {
				level_max = level_max.min(existing_level_max);
			}
			metadata.bbox_pyramid.set_level_max(level_max);
			tilejson.set_max_zoom(level_max);
		}

		if let Some(bbox) = args.bbox {
			let bbox = GeoBBox::try_from(&bbox)?;
			metadata.bbox_pyramid.intersect_geo_bbox(&bbox)?;
			if let Some(existing_bbox) = &mut tilejson.bounds {
				existing_bbox.intersect(&bbox);
			} else {
				tilejson.bounds = Some(bbox);
			}
			tilejson.center = None; // Center may no longer be valid after bbox intersection, so clear it to avoid confusion
		}

		let mask = if let Some(filename) = args.filename {
			let mask = factory.get_reader(DataLocation::try_from(&filename)?).await?;
			metadata.bbox_pyramid.intersect_pyramid(&mask.metadata().bbox_pyramid)?;
			Some(mask)
		} else {
			None
		};

		if metadata.bbox_pyramid.is_empty() {
			log::warn!(
				"Filter operation in VPL node {:?} results in empty bbox_pyramid",
				vpl_node.name
			);
		}

		metadata.update_tilejson(&mut tilejson);

		Ok(Self {
			metadata,
			source,
			mask,
			tilejson,
		})
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("filter", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("filter::get_tile_stream {bbox:?}");
		bbox.intersect_with_pyramid(&self.metadata.bbox_pyramid);
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		if let Some(mask) = &self.mask {
			let mut coord_stream = mask.get_tile_coord_stream(bbox).await?;
			let mut allowed = HashSet::new();
			while let Some((coord, _)) = coord_stream.next().await {
				allowed.insert(coord);
			}
			let source_stream = self.source.get_tile_stream(bbox).await?;
			Ok(source_stream.filter_coord(move |coord| {
				let contains = allowed.contains(&coord);
				async move { contains }
			}))
		} else {
			self.source.get_tile_stream(bbox).await
		}
	}

	async fn get_tile_coord_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		bbox.intersect_with_pyramid(&self.metadata.bbox_pyramid);
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}
		if let Some(mask) = &self.mask {
			mask.get_tile_coord_stream(bbox).await
		} else {
			self.source.get_tile_coord_stream(bbox).await
		}
	}
}

crate::operations::macros::define_transform_factory!("filter", Args, Operation);

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
	use super::*;
	use std::collections::HashSet;
	use versatiles_core::TileCoord;

	#[tokio::test]
	async fn test_filter_inside() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20]")
			.await?;

		let o = op.tilejson().as_object();
		assert_eq!(&o.get_number_array("bounds")?.unwrap(), &[0.0, 0.0, 40.0, 20.0]);
		assert_eq!(o.get_number("minzoom")?.unwrap(), 0.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 30.0);

		let inside: &[(u8, u32, u32)] = &[
			(0, 0, 0),
			(1, 1, 0),
			(2, 2, 1),
			(3, 4, 3),
			(4, 8, 7),
			(4, 9, 7),
			(5, 16, 14),
			(5, 16, 15),
			(5, 17, 14),
			(5, 17, 15),
			(5, 18, 14),
			(5, 18, 15),
			(5, 19, 14),
			(5, 19, 15),
		];
		let set: HashSet<(u8, u32, u32)> = inside.iter().copied().collect();

		for level in 0..=5 {
			let max_xy = 1 << level;
			for x in 0..max_xy {
				for y in 0..max_xy {
					let coord = TileCoord::new(level, x, y)?;
					let count = op.get_tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
					if set.contains(&(level, x, y)) {
						assert!(count == 1, "Expected one tile for {coord:?}, found {count}");
					} else {
						assert!(count == 0, "Expected no tiles for {coord:?}, found {count}");
					}
				}
			}
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_zoom_only() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter level_min=3 level_max=4")
			.await?;

		let o = op.tilejson().as_object();
		assert_eq!(
			o.get_number_array("bounds")?.unwrap(),
			[-180.0, -85.051129, 180.0, 85.051129]
		);
		assert_eq!(o.get_number("minzoom")?.unwrap(), 3.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 4.0);

		for z in 0..=6 {
			let coord = TileCoord::new(z, 0, 0)?;
			let n = op.get_tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
			assert_eq!(n == 1, (3..=4).contains(&z), "z={z}");
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_invalid_zoom_range_errors() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_debug format=mvt | filter level_min=5 level_max=2")
			.await;
		assert!(result.is_err(), "expected error for level_min > level_max");
	}

	#[tokio::test]
	async fn test_filter_composition_intersection_and_zoom_narrowing() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// First filter (wider), then a second filter that further restricts bbox + zooms
		let op = factory
			.operation_from_vpl(
				"from_debug format=mvt \
             | filter bbox=[0,0,60,30] level_min=1 level_max=28 \
             | filter bbox=[10,5,40,20] level_min=3 level_max=25",
			)
			.await?;

		let o = op.tilejson().as_object();

		// Expect the intersection of the two boxes
		let b: [f64; 4] = o.get_number_array("bounds")?.unwrap();
		assert!((b[0] - 10.0).abs() < 1e-4);
		assert!((b[1] - 5.0).abs() < 1e-4);
		assert!((b[2] - 40.0).abs() < 1e-4);
		assert!((b[3] - 20.0).abs() < 1e-4);

		// Expect the narrowed zoom range
		assert_eq!(o.get_number("minzoom")?.unwrap(), 3.0);
		assert_eq!(o.get_number("maxzoom")?.unwrap(), 25.0);

		// Sanity: tiles outside the final bbox shouldn't pass
		let outside = TileCoord::new(4, 0, 0)?.to_tile_bbox();
		let n_out = op.get_tile_stream(outside).await?.to_vec().await.len();
		assert_eq!(n_out, 0);

		// Inside tile at z=4 should pass
		let inside = TileCoord::new(4, 8, 7)?.to_tile_bbox(); // somewhere within [10,5,40,20]
		let n_in = op.get_tile_stream(inside).await?.to_vec().await.len();
		assert_eq!(n_in, 1);

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_filename_passes_matching_tiles() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// Both from_debug and the dummy "test.pbf" cover the full world, so all tiles pass.
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter filename=\"test.pbf\"")
			.await?;

		assert!(!op.metadata().bbox_pyramid.is_empty());

		let bbox = TileBBox::from_min_and_max(3, 1, 1, 2, 2)?;
		let count = op.get_tile_stream(bbox).await?.drain_and_count().await;
		assert!(count > 0, "Expected tiles to pass through filename filter");

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_filename_and_bbox_combined() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,10,10] filename=\"test.pbf\"")
			.await?;

		assert!(!op.metadata().bbox_pyramid.is_empty());

		// A tile far outside the bbox should not pass even though the mask is full-world.
		let far = TileCoord::new(3, 7, 7)?.to_tile_bbox();
		let count = op.get_tile_stream(far).await?.drain_and_count().await;
		assert_eq!(count, 0, "Expected no tiles outside filtered bbox");

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_filename_coord_stream_delegates_to_mask() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter filename=\"test.pbf\"")
			.await?;

		let bbox = TileBBox::from_min_and_max(2, 0, 0, 3, 3)?;
		let count = op.get_tile_coord_stream(bbox).await?.drain_and_count().await;
		assert!(count > 0);

		Ok(())
	}
}
