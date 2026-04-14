use anyhow::{Result, anyhow, ensure};
use imageproc::image::DynamicImage;
use moka::future::Cache;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{MAX_ZOOM_LEVEL, TileBBox, TileCoord, TileJSON, TilePyramid, TileStream};
use versatiles_image::GenericImage;

pub type ScaleDownFn = Arc<dyn Fn(&DynamicImage) -> Result<DynamicImage> + Send + Sync>;

#[derive(Clone)]
pub struct TileResizeCore {
	pub source: Arc<Box<dyn TileSource>>,
	pub metadata: TileSourceMetadata,
	pub tilejson: TileJSON,
	source_tile_size: u32,
	cache: Arc<Cache<TileCoord, Option<Arc<DynamicImage>>>>,
	scale_down_fn: ScaleDownFn,
}

impl Debug for TileResizeCore {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("TileResizeCore")
			.field("metadata", &self.metadata)
			.field("tilejson", &self.tilejson)
			.field("source_tile_size", &self.source_tile_size)
			.field("cache", &"<moka::future::Cache>")
			.finish()
	}
}

impl TileResizeCore {
	pub fn new(source: Box<dyn TileSource>, target_tile_size: u32, scale_down_fn: ScaleDownFn) -> Result<Self> {
		let source_tile_size = source
			.tilejson()
			.tile_size
			.map(|ts| u32::from(ts.size()))
			.ok_or_else(|| anyhow!("source tile_size is not set"))?;

		ensure!(
			target_tile_size == 256 || target_tile_size == 512,
			"target tile_size must be 256 or 512"
		);
		ensure!(
			source_tile_size != target_tile_size,
			"source tile_size ({source_tile_size}) must differ from target ({target_tile_size})"
		);

		let source_pyramid = source.metadata().bbox_pyramid.clone();
		let mut output_pyramid = TilePyramid::new_empty();

		let source_max = source_pyramid
			.level_max()
			.ok_or_else(|| anyhow!("source has no zoom levels"))?;

		match target_tile_size {
			256 => {
				// 512→256: split
				ensure!(
					source_tile_size == 512,
					"source tile_size must be 512 for 256 target (only 512→256 split supported)"
				);
				ensure!(
					source_max < MAX_ZOOM_LEVEL,
					"source max zoom level ({source_max}) must be below {MAX_ZOOM_LEVEL} for 512→256 conversion"
				);

				for level in 0..=source_max {
					let bbox = source_pyramid.get_level_bbox(level);
					if !bbox.is_empty() {
						if level == 0 {
							output_pyramid.set_level_bbox(TileBBox::new_full(0)?);
						}
						output_pyramid.set_level_bbox(bbox.leveled_up());
					}
				}
			}
			512 => {
				// 256→512: merge
				ensure!(
					source_tile_size == 256,
					"source tile_size must be 256 for 512 target (only 256→512 merge supported)"
				);
				ensure!(
					source_max >= 1,
					"source must have zoom levels >= 1 for 256→512 merge (need children to merge)"
				);

				for level in 1..=source_max {
					let bbox = source_pyramid.get_level_bbox(level);
					if !bbox.is_empty() {
						output_pyramid.set_level_bbox(bbox.leveled_down());
					}
				}
			}
			_ => unreachable!(),
		}

		let mut metadata = source.metadata().clone();
		metadata.bbox_pyramid = output_pyramid;

		let mut tilejson = source.tilejson().clone();
		tilejson.set_tile_size(target_tile_size)?;
		metadata.update_tilejson(&mut tilejson);

		let cache = Cache::builder()
			.max_capacity(512 * 1024 * 1024)
			.weigher(|_k: &TileCoord, v: &Option<Arc<DynamicImage>>| -> u32 {
				v.as_ref().map_or(8, |image| image.width() * image.height() * 4)
			})
			.build();

		Ok(Self {
			source: Arc::new(source),
			metadata,
			tilejson,
			source_tile_size,
			cache: Arc::new(cache),
			scale_down_fn,
		})
	}

	async fn fetch_source_tile(&self, coord: &TileCoord) -> Result<Option<Arc<DynamicImage>>> {
		if let Some(cached) = self.cache.get(coord).await {
			return Ok(cached);
		}

		let image = self
			.source
			.get_tile(coord)
			.await?
			.map(|t| t.into_image().map(Arc::new))
			.transpose()?;

		self.cache.insert(*coord, image.clone()).await;
		Ok(image)
	}

	async fn process_split_tile(&self, coord_dst: TileCoord) -> Result<Option<DynamicImage>> {
		if coord_dst.level == 0 {
			let source_coord = TileCoord::new(0, 0, 0)?;
			return self
				.fetch_source_tile(&source_coord)
				.await?
				.map(|image| (self.scale_down_fn)(&image))
				.transpose();
		}

		let source_coord = TileCoord::new(coord_dst.level - 1, coord_dst.x / 2, coord_dst.y / 2)?;
		if let Some(image) = self.fetch_source_tile(&source_coord).await? {
			let qx = coord_dst.x % 2;
			let qy = coord_dst.y % 2;
			return Ok(Some(image.crop_imm(qx * 256, qy * 256, 256, 256)));
		}
		Ok(None)
	}

	async fn process_merge_tile(&self, coord_dst: TileCoord) -> Result<Option<DynamicImage>> {
		let child_level = coord_dst.level + 1;
		let base_x = coord_dst.x * 2;
		let base_y = coord_dst.y * 2;

		let offsets: [(u32, u32); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];
		let mut children: Vec<(u32, u32, Option<DynamicImage>)> = Vec::with_capacity(4);

		for (dx, dy) in offsets {
			let child_coord = TileCoord::new(child_level, base_x + dx, base_y + dy)?;
			let child_image = self
				.source
				.get_tile(&child_coord)
				.await?
				.map(Tile::into_image)
				.transpose()?;
			children.push((dx, dy, child_image));
		}

		if children.iter().all(|(_, _, img)| img.is_none()) {
			return Ok(None);
		}

		let mut canvas = DynamicImage::new_rgba8(512, 512);
		for (dx, dy, child_image) in &children {
			if let Some(img) = child_image {
				canvas.copy_from(img, dx * 256, dy * 256)?;
			}
		}

		Ok(Some(canvas))
	}

	pub async fn get_tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.bbox_pyramid.intersected_bbox(&bbox)?;
		if bbox.is_empty() {
			return Ok(TileStream::empty());
		}

		let is_split = self.source_tile_size == 512;
		let mut coords = std::collections::HashSet::new();

		if is_split {
			// 512→256: each source tile at (level-1, x, y) produces up to 4
			// output tiles at (level, 2x..2x+1, 2y..2y+1), plus level 0 special case.
			if bbox.level() == 0 {
				// Level 0 output exists if source has (0,0,0)
				let mut stream = self.source.get_tile_coord_stream(TileBBox::new_full(0)?).await?;
				if stream.next().await.is_some() {
					coords.insert(TileCoord::new(0, 0, 0)?);
				}
			} else {
				let source_bbox = bbox.leveled_down();
				let mut stream = self.source.get_tile_coord_stream(source_bbox).await?;
				while let Some((src_coord, _)) = stream.next().await {
					// Each source coord produces up to 4 children
					for dy in 0..2u32 {
						for dx in 0..2u32 {
							let child = TileCoord::new(src_coord.level + 1, src_coord.x * 2 + dx, src_coord.y * 2 + dy)?;
							if bbox.includes_coord(&child)? {
								coords.insert(child);
							}
						}
					}
				}
			}
		} else {
			// 256→512: each source tile at (level+1, x, y) contributes to
			// output tile at (level, x/2, y/2). An output tile exists if any
			// of its 4 children exist.
			let source_bbox = bbox.leveled_up();
			let mut stream = self.source.get_tile_coord_stream(source_bbox).await?;
			while let Some((src_coord, _)) = stream.next().await {
				let parent = src_coord.to_level_decreased()?;
				if bbox.includes_coord(&parent)? {
					coords.insert(parent);
				}
			}
		}

		let vec: Vec<(TileCoord, ())> = coords.into_iter().map(|c| (c, ())).collect();
		Ok(TileStream::from_vec(vec))
	}

	pub fn get_tile_stream(&self, bbox_dst: TileBBox) -> Result<TileStream<'static, Tile>> {
		if !self.metadata.bbox_pyramid.intersects_bbox(&bbox_dst) {
			return Ok(TileStream::empty());
		}

		let self_arc = Arc::new(self.clone());
		let tile_format = self.metadata.tile_format;
		let is_split = self.source_tile_size == 512;

		let stream = TileStream::from_bbox_async_parallel(bbox_dst, move |coord_dst| {
			let self_arc = self_arc.clone();
			async move {
				let result = if is_split {
					self_arc.process_split_tile(coord_dst).await
				} else {
					self_arc.process_merge_tile(coord_dst).await
				};

				match result {
					Ok(Some(img)) => match Tile::from_image(img, tile_format) {
						Ok(tile) => Some((coord_dst, tile)),
						Err(e) => {
							log::error!("Error creating tile {coord_dst:?}: {e:?}");
							None
						}
					},
					Ok(None) => None,
					Err(e) => {
						log::error!("Error processing tile {coord_dst:?}: {e:?}");
						None
					}
				}
			}
		});

		Ok(stream)
	}
}
