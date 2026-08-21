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

use std::sync::Arc;

use anyhow::{Result, bail};
use blur_function::BlurFunction;
use mask_geometry::{MaskGeometry, TileClassification};
use versatiles_container::{DataLocation, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCoord, TileFormat, TilePyramid};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

use crate::{
	PipelineFactory,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Makes raster pixels outside a GeoJSON polygon transparent.
struct Args {
	/// Path to a GeoJSON file holding a Polygon or MultiPolygon.
	geojson: String,
	/// Distance in meters by which to grow the mask, or shrink it when negative. Defaults to `0`.
	buffer: Option<f32>,
	/// Width in meters of the soft transition at the mask edge. Defaults to `0`.
	blur: Option<f32>,
	/// Falloff curve across the `blur` band. Defaults to `linear`.
	blur_function: Option<BlurFunction>,
}

#[derive(Debug)]
struct Operation {
	mask: Arc<MaskGeometry>,
	/// The source pyramid clipped to the mask, so tiles entirely outside it are
	/// never requested. Computed in `build` because it needs the source's
	/// pyramid, which is async to obtain.
	tile_pyramid: Option<TilePyramid>,
}

impl Operation {
	#[context("Building raster_mask operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;

		// Validate source format is raster
		let metadata = source.metadata().clone();
		if !matches!(
			metadata.tile_format(),
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP
		) {
			bail!(
				"raster_mask requires a raster tile source, but got format: {:?}",
				metadata.tile_format()
			);
		}

		// Parse blur function
		let blur_function = args.blur_function.unwrap_or_default();

		// Resolve GeoJSON path relative to the factory's base path
		let geojson_path = factory
			.resolve_location(&DataLocation::try_from(&args.geojson)?)?
			.to_path_buf()?;

		// Load mask geometry
		let mask = MaskGeometry::from_geojson(
			&geojson_path,
			f64::from(args.buffer.unwrap_or(0.0)),
			f64::from(args.blur.unwrap_or(0.0)),
			blur_function,
		)?;

		// Clip tile_pyramid to the mask geometry's geographic bounds so that tiles
		// entirely outside the mask are never requested from the source.
		let tile_pyramid = match mask.geo_bbox() {
			Some(geo_bbox) => {
				let mut tile_pyramid = source.tile_pyramid().await?.as_ref().clone();
				tile_pyramid.intersect_geo_bbox(&geo_bbox)?;
				Some(tile_pyramid)
			}
			None => None,
		};

		let operation = Operation {
			mask: Arc::new(mask),
			tile_pyramid,
		};
		Ok(TransformOp::new(source, operation, factory.runtime()))
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "raster_mask";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		if let Some(tile_pyramid) = self.tile_pyramid.clone() {
			metadata.set_tile_pyramid(tile_pyramid);
		}
	}

	fn run(&self, coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		let tile_bbox = coord.to_mercator_bbox();

		match self.mask.classify_tile(tile_bbox) {
			TileClassification::FullyInside => Ok(Some(tile)),
			TileClassification::FullyOutside => Ok(None),
			TileClassification::Partial => {
				// Process using hierarchical subdivision for efficiency
				let format = tile.format();

				let image = tile.into_image()?;
				let (width, height) = (image.width(), image.height());
				let mut rgba = image.into_rgba8();

				// Compute alpha values using the hierarchical method (R-tree accelerated)
				let alpha_grid = self.mask.compute_alpha_grid(tile_bbox, width, height);

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

				Ok(Some(Tile::from_image(DynamicImage::ImageRgba8(rgba), format)?))
			}
		}
	}
}

crate::operations::macros::define_transform_factory!("raster_mask", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use assert_fs::prelude::*;
	use versatiles_core::TileCoord;

	use super::*;
	use crate::{PipelineFactory, factory::OperationFactoryTrait};

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

	// ─────────────────────── reference tests ───────────────────────
	//
	// These pin the alpha grid itself, which nothing did before: the tests above
	// assert only that a tile came back, and pass just as happily if the mask
	// returns nothing at all.
	//
	// They exist so `compute_alpha_grid` can be rewritten — scanline filling,
	// clipping the edge set per tile, computing distances only near the boundary
	// (see #251) — and the pixels can be shown not to have moved.
	//
	// The expectations are rendered as text rather than stored as a blob so a
	// reviewer can see the shape, and a failure shows *where* it changed rather
	// than that a hash differs.

	/// A geometry built to break the things a scanline rewrite gets wrong:
	/// a notch so a row crosses the boundary four times instead of two, an
	/// interior ring, and a second detached polygon.
	fn reference_geojson() -> assert_fs::NamedTempFile {
		let file = assert_fs::NamedTempFile::new("reference.geojson").unwrap();
		file
			.write_str(
				r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"reference"},
				"geometry":{"type":"MultiPolygon","coordinates":[
				[[[2,2],[18,2],[18,18],[12,18],[12,9],[8,9],[8,18],[2,18],[2,2]],
				 [[5,5],[9,5],[9,7],[5,7],[5,5]]],
				[[[20,12],[24,12],[24,16],[20,16],[20,12]]]]}}]}"#,
			)
			.unwrap();
		file
	}

	/// One character per `step`×`step` block of the alpha grid, by mean value.
	fn render(alpha: &[u8], width: u32, step: usize) -> String {
		// No space in the ramp: trailing whitespace in a raw string is silently
		// eaten by editors and formatters, which would corrupt the expectations.
		const RAMP: &[u8] = b".:-=+*#%@";
		let width = width as usize;
		let height = alpha.len() / width;
		let mut out = String::new();
		for by in (0..height).step_by(step) {
			for bx in (0..width).step_by(step) {
				let mut sum = 0usize;
				let mut n = 0usize;
				for y in by..(by + step).min(height) {
					for x in bx..(bx + step).min(width) {
						sum += alpha[y * width + x] as usize;
						n += 1;
					}
				}
				let mean = sum / n.max(1);
				out.push(RAMP[mean * (RAMP.len() - 1) / 255] as char);
			}
			out.push('\n');
		}
		out
	}

	/// Counts that a coarse rendering cannot see: how many pixels are fully
	/// transparent, fully opaque, and somewhere in between.
	// clippy suggests the `bytecount` crate; a whole dependency to count 65k
	// bytes once per test is not a trade worth making.
	#[allow(clippy::naive_bytecount)]
	fn tally(alpha: &[u8]) -> (usize, usize, usize) {
		let clear = alpha.iter().filter(|&&a| a == 0).count();
		let solid = alpha.iter().filter(|&&a| a == 255).count();
		(clear, solid, alpha.len() - clear - solid)
	}

	fn reference_mask(buffer: f64, blur: f64) -> MaskGeometry {
		let file = reference_geojson();
		MaskGeometry::from_geojson(file.path(), buffer, blur, BlurFunction::default()).unwrap()
	}

	#[test]
	fn the_reference_shape_is_unchanged() {
		// The whole geometry in one tile: the notch as a gap between two arms
		// (rows 3-9), the island on the right (rows 4-7), the interior ring
		// (rows 11-12), the southern edge (row 14).
		let mask = reference_mask(0.0, 0.0);
		let coord = TileCoord::new(4, 8, 7).unwrap();
		let alpha = mask.compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);

		assert_eq!(
			render(&alpha, 256, 16),
			concat!(
				"................\n",
				"................\n",
				"................\n",
				".+%%%*..=%%%#...\n",
				".+@@@*..=@@@#.=+\n",
				".+@@@*..=@@@#.#@\n",
				".+@@@*..=@@@#.#@\n",
				".+@@@*..=@@@#.-=\n",
				".+@@@*..=@@@#...\n",
				".+@@@#==*@@@#...\n",
				".+@%%%%@@@@@#...\n",
				".+@+..+@@@@@#...\n",
				".+@#++#@@@@@#...\n",
				".+@@@@@@@@@@#...\n",
				".-++++++++++=...\n",
				"................\n",
			)
		);
		// The rendering averages 16x16 blocks, so it cannot see a handful of
		// changed pixels along an edge. These counts can.
		assert_eq!(tally(&alpha), (35259, 27914, 2363), "(clear, solid, partial)");
	}

	#[test]
	fn a_tile_well_inside_is_fully_opaque() {
		let mask = reference_mask(0.0, 0.0);
		let coord = TileCoord::from_geo(15.0, 15.0, 8).unwrap();
		let alpha = mask.compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);
		assert_eq!(tally(&alpha), (0, 65536, 0));
	}

	#[test]
	fn a_tile_inside_the_interior_ring_is_fully_transparent() {
		// The case a scanline fill gets wrong by counting the hole's edges as
		// entries rather than exits.
		let mask = reference_mask(0.0, 0.0);
		let coord = TileCoord::from_geo(7.0, 6.0, 9).unwrap();
		let alpha = mask.compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);
		assert_eq!(tally(&alpha), (65536, 0, 0));
	}

	#[test]
	fn a_tile_beyond_the_mask_is_fully_transparent() {
		let mask = reference_mask(0.0, 0.0);
		let coord = TileCoord::from_geo(30.0, 30.0, 6).unwrap();
		let alpha = mask.compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);
		assert_eq!(tally(&alpha), (65536, 0, 0));
	}

	#[test]
	fn blur_widens_the_transition_band() {
		let coord = TileCoord::new(4, 8, 7).unwrap();
		let sharp = reference_mask(0.0, 0.0).compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);
		let blurred = reference_mask(0.0, 60_000.0).compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);

		// Without blur the only partial pixels are the one-pixel antialiasing
		// band; with it, the transition is many pixels wide.
		assert_eq!(tally(&sharp).2, 2363);
		assert_eq!(tally(&blurred).2, 14336);
	}

	#[test]
	fn a_positive_buffer_grows_the_covered_area() {
		let coord = TileCoord::new(4, 8, 7).unwrap();
		let plain = reference_mask(0.0, 0.0).compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);
		let grown = reference_mask(80_000.0, 0.0).compute_alpha_grid(coord.to_mercator_bbox(), 256, 256);

		let (plain_clear, plain_solid, _) = tally(&plain);
		let (grown_clear, grown_solid, _) = tally(&grown);
		assert!(grown_solid > plain_solid, "{grown_solid} should exceed {plain_solid}");
		assert!(grown_clear < plain_clear, "{grown_clear} should be under {plain_clear}");
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_mask");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
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
			.operation_from_vpl(&format!("from_debug format=png | raster_mask geojson='{geojson_path}'"))
			.await?;

		// Get a tile that should be affected
		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.tile_stream(coord.to_tile_bbox()).await?;
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
				"from_debug format=png | raster_mask geojson='{geojson_path}' buffer=1000"
			))
			.await?;

		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.tile_stream(coord.to_tile_bbox()).await?;
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
				"from_debug format=png | raster_mask geojson='{geojson_path}' blur=500 blur_function=cosine"
			))
			.await?;

		let coord = TileCoord::new(2, 2, 1)?;
		let stream = op.tile_stream(coord.to_tile_bbox()).await?;
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
			.operation_from_vpl(&format!("from_debug format=mvt | raster_mask geojson='{geojson_path}'"))
			.await;

		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let geojson_file = create_test_geojson();
		let geojson_path = geojson_file.path().display();

		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!("from_debug format=png | raster_mask geojson='{geojson_path}'"))
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
			.operation_from_vpl(&format!("from_debug format=png | raster_mask geojson='{geojson_path}'"))
			.await?;

		// Metadata should be passed through from source
		let metadata = op.metadata();
		assert_eq!(*metadata.tile_format(), TileFormat::PNG);

		Ok(())
	}
}
