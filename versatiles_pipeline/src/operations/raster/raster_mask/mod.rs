//! Raster mask pipeline operation.
//!
//! This operation applies a polygonal mask from GeoJSON to raster tiles.
//! Pixels outside the polygon become transparent.
//!
//! # Example
//!
//! ```text
//! from_container file=./tiles.versatiles | raster_mask geojson=./germany.geojson buffer=1000 blur=500 blur_function=cosine
//! ```

mod blur_function;
mod mask_geometry;

use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use blur_function::BlurFunction;
use mask_geometry::{MaskGeometry, TileClassification};
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileFormat, TileJSON, TileStream};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Apply a polygon mask from GeoJSON to raster tiles.
/// Pixels outside the polygon become transparent.
struct Args {
	/// Path to GeoJSON file with Polygon or MultiPolygon geometry.
	geojson: String,
	/// Buffer distance in meters. Positive values expand the mask, negative values shrink it.
	/// Default: 0
	buffer: Option<f32>,
	/// Edge blur distance in meters. Creates a soft transition at the mask edge.
	/// Default: 0
	blur: Option<f32>,
	/// Blur falloff function: "linear" or "cosine".
	/// Default: "linear"
	blur_function: Option<String>,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	mask: Arc<MaskGeometry>,
}

impl Operation {
	#[context("Building raster_mask operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		// Validate source format is raster
		let metadata = source.metadata().clone();
		if !matches!(
			metadata.tile_format,
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP
		) {
			bail!(
				"raster_mask requires a raster tile source, but got format: {:?}",
				metadata.tile_format
			);
		}

		// Parse blur function
		let blur_function = if let Some(bf) = args.blur_function {
			BlurFunction::try_from(bf.as_str())?
		} else {
			BlurFunction::default()
		};

		// Resolve GeoJSON path relative to the factory's base path
		let geojson_path = factory.resolve_path(&args.geojson);

		// Load mask geometry
		let mask = MaskGeometry::from_geojson(
			&geojson_path,
			f64::from(args.buffer.unwrap_or(0.0)),
			f64::from(args.blur.unwrap_or(0.0)),
			blur_function,
		)?;

		let tilejson = source.tilejson().clone();

		Ok(Self {
			source,
			metadata,
			tilejson,
			mask: Arc::new(mask),
		})
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_mask", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");

		let mask = Arc::clone(&self.mask);
		let stream = self.source.get_tile_stream(bbox).await?;

		Ok(stream
			.filter_map_parallel_try(move |coord, tile| {
				let tile_bbox = coord.to_mercator_bbox();
				let classification = mask.classify_tile(tile_bbox);

				match classification {
					TileClassification::FullyInside => {
						// Pass through unchanged
						Ok(Some(tile))
					}
					TileClassification::FullyOutside => {
						// Skip this tile entirely
						Ok(None)
					}
					TileClassification::Partial => {
						// Process using hierarchical subdivision for efficiency
						let format = tile.format();

						let image = tile.into_image()?;
						let (width, height) = (image.width(), image.height());
						let mut rgba = image.into_rgba8();

						// Compute alpha grid using hierarchical method (R-tree accelerated)
						let alpha_grid = mask.compute_alpha_grid(tile_bbox, width, height);

						// Apply alpha values to pixels
						for py in 0..height {
							for px in 0..width {
								let mask_alpha = alpha_grid[(py * width + px) as usize];
								let pixel = rgba.get_pixel_mut(px, py);
								// Multiply existing alpha with mask alpha
								#[allow(clippy::cast_possible_truncation)]
								{
									pixel[3] = ((u16::from(pixel[3]) * u16::from(mask_alpha)) / 255) as u8;
								}
							}
						}

						// Convert back to the original format
						let dynamic_image = DynamicImage::ImageRgba8(rgba);
						let new_tile = Tile::from_image(dynamic_image, format)?;

						Ok(Some(new_tile))
					}
				}
			})
			.unwrap_results())
	}
}

crate::operations::macros::define_transform_factory!("raster_mask", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::PipelineFactory;
	use crate::factory::OperationFactoryTrait;
	use assert_fs::prelude::*;
	use versatiles_core::TileCoord;

	fn create_test_geojson() -> assert_fs::NamedTempFile {
		let file = assert_fs::NamedTempFile::new("test.geojson").unwrap();
		file
			.write_str(
				r#"{
				"type": "FeatureCollection",
				"features": [{
					"type": "Feature",
					"geometry": {
						"type": "Polygon",
						"coordinates": [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]
					},
					"properties": {}
				}]
			}"#,
			)
			.unwrap();
		file
	}

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "raster_mask");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("geojson"));
		assert!(docs.contains("buffer"));
		assert!(docs.contains("blur"));
	}

	#[tokio::test]
	async fn test_raster_mask_basic() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_mask geojson=\"{geojson_path}\""
			))
			.await?;

		// Get a tile that should be affected
		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.get_tile_stream(coord.to_tile_bbox()).await?;
		let tiles: Vec<_> = stream.to_vec().await;

		// Should have tiles (some may be filtered if outside mask)
		// The exact count depends on the mask geometry
		assert!(tiles.len() <= 1);

		Ok(())
	}

	#[tokio::test]
	async fn test_raster_mask_with_buffer() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_mask geojson=\"{geojson_path}\" buffer=1000"
			))
			.await?;

		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.get_tile_stream(coord.to_tile_bbox()).await?;
		let _tiles: Vec<_> = stream.to_vec().await;

		Ok(())
	}

	#[tokio::test]
	async fn test_raster_mask_with_blur() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_mask geojson=\"{geojson_path}\" blur=500 blur_function=cosine"
			))
			.await?;

		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.get_tile_stream(coord.to_tile_bbox()).await?;
		let _tiles: Vec<_> = stream.to_vec().await;

		Ok(())
	}

	#[tokio::test]
	async fn test_raster_mask_invalid_format() {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		// MVT is vector, not raster
		let result = factory
			.operation_from_vpl(&format!(
				"from_debug format=mvt | raster_mask geojson=\"{geojson_path}\""
			))
			.await;

		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_mask geojson=\"{geojson_path}\""
			))
			.await?;

		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_mask"));

		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_passthrough() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_mask geojson=\"{geojson_path}\""
			))
			.await?;

		// Metadata should be passed through from source
		let metadata = op.metadata();
		assert_eq!(metadata.tile_format, TileFormat::PNG);

		Ok(())
	}
}
