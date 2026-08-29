use std::{collections::HashSet, fmt::Debug, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{GeoBBox, GeoCrop, TileBBox, TileJSON, TileStream, ZoomLevel};
use versatiles_derive::context;

use crate::{PipelineFactory, helpers::location::SourceLocation, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filters tiles by bounding box, zoom range, or the coordinates present in another container.
///
/// Every parameter narrows the tile set, except `bbox_border`, which widens it.
/// A `bbox_border=2` keeps a ring of tiles the bbox alone would have dropped;
/// those tiles lie outside the crop, so the advertised bounds are extended to
/// cover them and a client actually requests them.
///
/// That ring matters wherever a cropped tileset is rendered rather than just
/// stored: without it, labels and geometry near the edge have no neighbouring
/// tiles to be laid out against.
///
/// `filename` takes the same path and URL syntax as `from_container`. Opening
/// it to build the allow-list costs I/O when the pipeline is built, unlike
/// every other parameter here.
struct Args {
	/// Area to keep, in WGS84 degrees. Defaults to the source's own bounds.
	bbox: Option<GeoBBox>,
	/// Ring of extra tiles kept around `bbox`, per zoom level. Requires `bbox`. Defaults to `0`.
	#[vpl(default = "0")]
	bbox_border: Option<u32>,
	/// Lowest zoom level to keep. Defaults to the source's lowest.
	level_min: Option<ZoomLevel>,
	/// Highest zoom level to keep. Defaults to the source's highest.
	level_max: Option<ZoomLevel>,
	/// Tile container whose coordinates act as an allow-list. Defaults to no allow-list.
	#[vpl(accepts = "versatiles, mbtiles, pmtiles, tar")]
	filename: Option<SourceLocation>,
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

		// Out of the type and into the `u8` the pyramid and the tilejson take:
		// the level's range is checked where it is parsed, not here.
		let (arg_level_min, arg_level_max) = (args.level_min.map(u8::from), args.level_max.map(u8::from));

		if let (Some(lo), Some(hi)) = (arg_level_min, arg_level_max)
			&& lo > hi
		{
			bail!(
				"Invalid zoom range in filter node {:?}: level_min ({lo}) must be ≤ level_max ({hi})",
				vpl_node.name
			);
		}

		let mut tile_pyramid = source.tile_pyramid().await?.as_ref().clone();

		if let Some(mut level_min) = arg_level_min {
			if let Some(existing_level_min) = tile_pyramid.level_min() {
				level_min = level_min.max(existing_level_min);
			}
			tile_pyramid.set_level_min(level_min);
			tilejson.set_zoom_min(level_min);
		}

		if let Some(mut level_max) = arg_level_max {
			if let Some(existing_level_max) = tile_pyramid.level_max() {
				level_max = level_max.min(existing_level_max);
			}
			tile_pyramid.set_level_max(level_max);
			tilejson.set_zoom_max(level_max);
		}

		if args.bbox.is_none() && args.bbox_border.is_some() {
			bail!(
				"Invalid arguments in filter node {:?}: bbox_border has nothing to widen without bbox",
				vpl_node.name
			);
		}

		if let Some(bbox) = args.bbox {
			let crop = GeoCrop::new(bbox, args.bbox_border.unwrap_or(0));
			crop.apply_to_pyramid(&mut tile_pyramid)?;

			tilejson.bounds = Some(crop.bounds(tilejson.bounds, &tile_pyramid));
			tilejson.center = None; // Center may no longer be valid after cropping, so clear it to avoid confusion
		}

		let mask = if let Some(filename) = args.filename {
			let mask = factory.reader(filename.to_location()?).await?;
			let mask_pyramid = mask.tile_pyramid().await?;
			tile_pyramid.intersect_pyramid(&mask_pyramid);
			Some(mask)
		} else {
			None
		};

		if tile_pyramid.is_empty() {
			log::warn!(
				"Filter operation in VPL node {:?} results in empty tile_pyramid",
				vpl_node.name
			);
		}

		metadata.set_tile_pyramid(tile_pyramid);
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

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("filter::tile_stream {bbox:?}");
		let bbox = self.metadata.intersection_bbox(&bbox);
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		if let Some(mask) = &self.mask {
			let mut coord_stream = mask.tile_coord_stream(bbox).await?;
			let mut allowed = HashSet::new();
			while let Some((coord, _)) = coord_stream.next().await {
				allowed.insert(coord);
			}
			let source_stream = self.source.tile_stream(bbox).await?;
			Ok(source_stream.filter_coord(move |coord| {
				let contains = allowed.contains(&coord);
				async move { contains }
			}))
		} else {
			self.source.tile_stream(bbox).await
		}
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}
		if let Some(mask) = &self.mask {
			mask.tile_coord_stream(bbox).await
		} else {
			self.source.tile_coord_stream(bbox).await
		}
	}
}

crate::operations::macros::define_transform_factory!("filter", Args, Operation);

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use approx::assert_relative_eq;
	use versatiles_core::{TileCoord, TilePyramid};

	use super::*;

	#[tokio::test]
	async fn test_filter_inside() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20]")
			.await?;

		let o = op.tilejson().as_object();
		assert_relative_eq!(
			o.number_array::<4>("bounds")?.unwrap().as_slice(),
			[0.0_f64, 0.0, 40.0, 20.0].as_slice()
		);
		assert_relative_eq!(o.number("minzoom")?.unwrap(), 0.0);
		assert_relative_eq!(o.number("maxzoom")?.unwrap(), 30.0);

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
					let count = op.tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
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
		assert_relative_eq!(
			o.number_array::<4>("bounds")?.unwrap().as_slice(),
			[-180.0_f64, -85.051129, 180.0, 85.051129].as_slice()
		);
		assert_relative_eq!(o.number("minzoom")?.unwrap(), 3.0);
		assert_relative_eq!(o.number("maxzoom")?.unwrap(), 4.0);

		for z in 0..=6 {
			let coord = TileCoord::new(z, 0, 0)?;
			let n = op.tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len();
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
		let b: [f64; 4] = o.number_array("bounds")?.unwrap();
		assert!((b[0] - 10.0).abs() < 1e-4);
		assert!((b[1] - 5.0).abs() < 1e-4);
		assert!((b[2] - 40.0).abs() < 1e-4);
		assert!((b[3] - 20.0).abs() < 1e-4);

		// Expect the narrowed zoom range
		assert_relative_eq!(o.number("minzoom")?.unwrap(), 3.0);
		assert_relative_eq!(o.number("maxzoom")?.unwrap(), 25.0);

		// Sanity: tiles outside the final bbox shouldn't pass
		let outside = TileCoord::new(4, 0, 0)?.to_tile_bbox();
		let n_out = op.tile_stream(outside).await?.to_vec().await.len();
		assert_eq!(n_out, 0);

		// Inside tile at z=4 should pass
		let inside = TileCoord::new(4, 8, 7)?.to_tile_bbox(); // somewhere within [10,5,40,20]
		let n_in = op.tile_stream(inside).await?.to_vec().await.len();
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

		assert!(!op.metadata().tile_pyramid().unwrap().is_empty());

		let bbox = TileBBox::from_min_and_max(3, 1, 1, 2, 2)?;
		let count = op.tile_stream(bbox).await?.drain_and_count().await;
		assert!(count > 0, "Expected tiles to pass through filename filter");

		Ok(())
	}

	#[tokio::test]
	async fn test_filter_filename_and_bbox_combined() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,10,10] filename=\"test.pbf\"")
			.await?;

		assert!(!op.metadata().tile_pyramid().unwrap().is_empty());

		// A tile far outside the bbox should not pass even though the mask is full-world.
		let far = TileCoord::new(3, 7, 7)?.to_tile_bbox();
		let count = op.tile_stream(far).await?.drain_and_count().await;
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
		let count = op.tile_coord_stream(bbox).await?.drain_and_count().await;
		assert!(count > 0);

		Ok(())
	}

	/// `bbox_border` must keep tiles the bbox alone drops, at every level.
	#[tokio::test]
	async fn bbox_border_widens_the_pyramid() -> Result<()> {
		let factory = PipelineFactory::new_dummy();

		let without = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5")
			.await?
			.tile_pyramid()
			.await?;
		let with = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5 bbox_border=2")
			.await?
			.tile_pyramid()
			.await?;

		assert!(
			with.count_tiles() > without.count_tiles(),
			"a border must add tiles: {} vs {}",
			without.count_tiles(),
			with.count_tiles()
		);

		// Every tile inside the crop is still there — a border only adds.
		for level in 0..=5u8 {
			let inner = without.level_ref(level).to_bbox();
			if inner.is_empty() {
				continue;
			}
			let outer = with.level_ref(level).to_bbox();
			assert!(
				outer.includes_bbox(&inner),
				"level {level}: border must not drop tiles the crop kept"
			);
		}
		Ok(())
	}

	/// The advertised bounds have to grow with the border, or a client selecting
	/// tiles by bbox overlap never requests the ring that was just written.
	#[tokio::test]
	async fn bbox_border_extends_the_advertised_bounds() -> Result<()> {
		let factory = PipelineFactory::new_dummy();

		let plain = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5")
			.await?;
		let bordered = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5 bbox_border=2")
			.await?;

		let plain_bounds = plain.tilejson().as_object().number_array::<4>("bounds")?.unwrap();
		let border_bounds = bordered.tilejson().as_object().number_array::<4>("bounds")?.unwrap();

		assert_relative_eq!(plain_bounds.as_slice(), [0.0_f64, 0.0, 40.0, 20.0].as_slice());
		assert!(border_bounds[0] < plain_bounds[0], "west must extend");
		assert!(border_bounds[1] < plain_bounds[1], "south must extend");
		assert!(border_bounds[2] > plain_bounds[2], "east must extend");
		assert!(border_bounds[3] > plain_bounds[3], "north must extend");
		Ok(())
	}

	/// `bbox_border=0` is the documented no-op, so it must not widen anything.
	#[tokio::test]
	async fn bbox_border_zero_changes_nothing() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let plain = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5")
			.await?;
		let zero = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox=[0,0,40,20] level_max=5 bbox_border=0")
			.await?;

		assert_eq!(
			plain.tile_pyramid().await?.count_tiles(),
			zero.tile_pyramid().await?.count_tiles()
		);
		assert_relative_eq!(
			plain
				.tilejson()
				.as_object()
				.number_array::<4>("bounds")?
				.unwrap()
				.as_slice(),
			zero
				.tilejson()
				.as_object()
				.number_array::<4>("bounds")?
				.unwrap()
				.as_slice()
		);
		Ok(())
	}

	/// `filter bbox_border=N` must describe the same tileset as
	/// `convert --bbox-border N`. The two implementations are separate, so this
	/// pins them together — the whole point of #219 was that the CLI could
	/// express something a pipeline could not.
	#[tokio::test]
	async fn bbox_border_matches_the_converter() -> Result<()> {
		use versatiles_container::{TilesConvertReader, TilesConverterParameters};

		const BBOX: [f64; 4] = [13.3, 52.4, 13.5, 52.6];
		const BORDER: u32 = 2;
		const LEVEL_MAX: u8 = 12;

		let factory = PipelineFactory::new_dummy();

		// What `versatiles convert --bbox ... --bbox-border 2 --max-zoom 12` builds.
		let mut cli_pyramid = TilePyramid::new_full();
		cli_pyramid.set_level_max(LEVEL_MAX);
		cli_pyramid.intersect_geo_bbox(&GeoBBox::try_from(&BBOX)?)?;
		cli_pyramid.buffer(BORDER);
		let converter = TilesConvertReader::new_from_reader(
			std::sync::Arc::from(factory.operation_from_vpl("from_debug format=mvt").await?),
			TilesConverterParameters {
				tile_pyramid: Some(cli_pyramid),
				geo_bbox: Some(GeoBBox::try_from(&BBOX)?),
				bbox_border: Some(BORDER),
				..Default::default()
			},
		)
		.await?;

		let filtered = factory
			.operation_from_vpl(&format!(
				"from_debug format=mvt | filter bbox=[13.3,52.4,13.5,52.6] level_max={LEVEL_MAX} bbox_border={BORDER}"
			))
			.await?;

		assert_eq!(
			filtered.tile_pyramid().await?.as_ref(),
			converter.tile_pyramid().await?.as_ref(),
			"filter and convert must select the same tiles"
		);
		assert_relative_eq!(
			filtered
				.tilejson()
				.as_object()
				.number_array::<4>("bounds")?
				.unwrap()
				.as_slice(),
			converter
				.tilejson()
				.as_object()
				.number_array::<4>("bounds")?
				.unwrap()
				.as_slice()
		);
		Ok(())
	}

	/// Without a bbox there is nothing to widen. Failing beats silently ignoring
	/// the parameter, which is what the CLI flag does today.
	#[tokio::test]
	async fn bbox_border_without_bbox_is_an_error() {
		let factory = PipelineFactory::new_dummy();
		let err = factory
			.operation_from_vpl("from_debug format=mvt | filter bbox_border=2")
			.await
			.unwrap_err();
		assert!(
			format!("{err:#}").contains("bbox_border has nothing to widen without bbox"),
			"unexpected error: {err:#}"
		);
	}
}
