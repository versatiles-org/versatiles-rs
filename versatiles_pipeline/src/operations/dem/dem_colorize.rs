use anyhow::{Result, bail};
use imageproc::image::{RgbImage, RgbaImage};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCoord, TileFormat, TileJSON, TileSchema};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

use super::{
	color_ramp::ColorRamp,
	encoding::{DemEncoding, decode_elevation, resolve_encoding},
};
use crate::{
	PipelineFactory,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

/// The ramp `dem_colorize` uses without `ramp=`: a hypsometric tint, light
/// and muted so that a hillshade on top of it stays legible. The coast lies
/// just above 0, because DEMs without bathymetry, such as Mapterhorn's, store
/// the sea as exactly 0 m.
const DEFAULT_RAMP: &str = "-11000:24507e,-200:8cb4d8,0:bcd8ec,\
	0.01:a8c896,300:cad6a2,800:e4dcae,1500:d6bf98,2500:bea58a,4000:e4e0dc,6000:ffffff";

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Colours DEM tiles by elevation, the way MapLibre's `color-relief` layer colours them.
///
/// The ramp is a list of stops, `elevation:colour`, ascending by elevation in
/// meters, such as `ramp="0:00a000,1000:c8b464,3000:ffffff"`. Between two
/// stops the colour is interpolated linearly, and outside the ramp it is the
/// colour of the nearest end. A colour is `RRGGBB` or, translucent,
/// `RRGGBBAA`; a ramp with a translucent stop gives RGBA tiles, and one without
/// gives RGB tiles. A JPEG source becomes PNG when the ramp is translucent.
///
/// The default ramp is a hypsometric tint: shades of blue up to sea level,
/// then green, yellow and brown up to white peaks. Sea level itself is blue,
/// for DEMs that store the sea as 0 m, and so is land below it:
/// `-11000:24507e,-200:8cb4d8,0:bcd8ec,0.01:a8c896,300:cad6a2,800:e4dcae,1500:d6bf98,2500:bea58a,4000:e4e0dc,6000:ffffff`.
///
/// For a coloured relief, put a `dem_hillshade` on top with `from_stacked_raster`,
/// which lays its first source over the others.
struct Args {
	/// Colour stops, as `elevation:colour,…`. Defaults to a hypsometric tint.
	ramp: Option<ColorRamp>,
	/// DEM encoding of the source. Defaults to the encoding its tile schema implies.
	encoding: Option<DemEncoding>,
}

#[derive(Debug)]
struct Operation {
	ramp: ColorRamp,
	encoding: DemEncoding,
	/// The source's format, or PNG for a JPEG source the ramp makes translucent.
	format: TileFormat,
}

impl Operation {
	#[context("Building dem_colorize operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let ramp = match args.ramp {
			Some(ramp) => ramp,
			None => ColorRamp::try_from(DEFAULT_RAMP)?,
		};
		let mut format = *source.metadata().tile_format();
		if format == TileFormat::JPG && !ramp.is_opaque() {
			format = TileFormat::PNG;
		}
		let operation = Operation {
			ramp,
			// Read from the source's schema before `update_tilejson` overwrites it.
			encoding: resolve_encoding(args.encoding, source.tilejson().tile_schema.as_ref())?,
			format,
		};
		Ok(TransformOp::new(source, operation, factory.runtime()))
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "dem_colorize";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		metadata.set_tile_format(self.format);
	}

	fn update_tilejson(&self, tilejson: &mut TileJSON) {
		tilejson.tile_schema = Some(if self.ramp.is_opaque() {
			TileSchema::RasterRGB
		} else {
			TileSchema::RasterRGBA
		});
	}

	fn run(&self, _coord: &TileCoord, tile: Tile) -> Result<Option<Tile>> {
		let image = tile.into_image()?;
		let (width, height) = (image.width(), image.height());
		let heights: Vec<f64> = match image {
			DynamicImage::ImageRgb8(image) => image.pixels().map(|p| decode_elevation(self.encoding, p.0)).collect(),
			DynamicImage::ImageRgba8(image) => image
				.pixels()
				.map(|p| decode_elevation(self.encoding, [p.0[0], p.0[1], p.0[2]]))
				.collect(),
			_ => bail!("dem_colorize requires RGB8 or RGBA8 images"),
		};
		let colors = heights.into_iter().map(|meters| self.ramp.color_at(meters));
		let image = if self.ramp.is_opaque() {
			let bytes = colors.flat_map(|[r, g, b, _]| [r, g, b]).collect();
			DynamicImage::ImageRgb8(RgbImage::from_raw(width, height, bytes).expect("three bytes per pixel"))
		} else {
			let bytes = colors.flatten().collect();
			DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, bytes).expect("four bytes per pixel"))
		};
		Ok(Some(Tile::from_image(image, self.format)?))
	}
}

crate::operations::macros::define_transform_factory!("dem_colorize", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use versatiles_core::TileCoord;

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "test values fit 24 bits, and each byte is masked out of them"
	)]
	fn encode(encoding: DemEncoding, meters: f64) -> [u8; 3] {
		let raw = match encoding {
			DemEncoding::Mapbox => ((meters + 10000.0) * 10.0).round(),
			DemEncoding::Terrarium => ((meters + 32768.0) * 256.0).round(),
		} as u32;
		[(raw >> 16) as u8, (raw >> 8) as u8, raw as u8]
	}

	/// A source of one tile per elevation, all at zoom 3, row 0: tile `x`
	/// holds `meters[x]` everywhere.
	fn source(encoding: DemEncoding, format: TileFormat, meters: &'static [f64]) -> Result<DummyImageSource> {
		let mut source = DummyImageSource::new(
			move |coord| {
				let height = meters.get(coord.x as usize)?;
				let image = RgbImage::from_pixel(256, 256, imageproc::image::Rgb(encode(encoding, *height)));
				Tile::from_image(DynamicImage::ImageRgb8(image), TileFormat::PNG).ok()
			},
			format,
			None,
		)?;
		source.tilejson_mut().tile_schema = Some(super::super::encoding::to_tile_schema(encoding));
		Ok(source)
	}

	async fn colorize(source: Result<DummyImageSource>, vpl: &str) -> Result<TransformOp<Operation>> {
		Operation::build(
			VPLNode::try_from_str(vpl)?,
			Box::new(source?),
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	async fn pixel(op: &TransformOp<Operation>, x: u32) -> Result<[u8; 4]> {
		let tile = op.tile(&TileCoord::new(3, x, 0)?).await?.expect("a tile");
		Ok(tile.into_image()?.to_rgba8().get_pixel(17, 42).0)
	}

	#[test]
	fn factory_docs_name_the_parameters() {
		let docs = Factory {}.docs();
		for name in ["ramp", "encoding"] {
			assert!(docs.contains(name), "{name} missing from {docs}");
		}
	}

	#[test]
	fn the_default_ramp_parses_and_is_opaque() {
		assert!(ColorRamp::try_from(DEFAULT_RAMP).unwrap().is_opaque());
		assert!(
			Args::docs().contains(DEFAULT_RAMP),
			"the documented default ramp has drifted from the one in use"
		);
	}

	#[tokio::test]
	async fn elevations_take_the_colours_of_the_ramp() -> Result<()> {
		let op = colorize(
			source(
				DemEncoding::Terrarium,
				TileFormat::PNG,
				&[-500.0, 0.0, 50.0, 100.0, 5000.0],
			),
			"dem_colorize ramp=\"0:000000,100:FF8000\"",
		)
		.await?;
		assert_eq!(pixel(&op, 0).await?, [0, 0, 0, 255]);
		assert_eq!(pixel(&op, 1).await?, [0, 0, 0, 255]);
		assert_eq!(pixel(&op, 2).await?, [128, 64, 0, 255]);
		assert_eq!(pixel(&op, 3).await?, [255, 128, 0, 255]);
		assert_eq!(pixel(&op, 4).await?, [255, 128, 0, 255]);
		Ok(())
	}

	#[tokio::test]
	async fn both_encodings_colour_alike() -> Result<()> {
		let vpl = "dem_colorize ramp=\"-100:0000FF,0:00FF00,1000:FF0000\"";
		let mapbox = colorize(source(DemEncoding::Mapbox, TileFormat::PNG, &[-50.0, 250.0]), vpl).await?;
		let terrarium = colorize(source(DemEncoding::Terrarium, TileFormat::PNG, &[-50.0, 250.0]), vpl).await?;
		for x in 0..2 {
			assert_eq!(pixel(&mapbox, x).await?, pixel(&terrarium, x).await?);
		}
		Ok(())
	}

	/// The schema follows from the ramp: opaque stops give RGB, a translucent
	/// stop RGBA; and only a translucent ramp has to leave JPEG.
	#[tokio::test]
	async fn the_ramp_decides_rgb_or_rgba_and_whether_jpeg_stays() -> Result<()> {
		let opaque = colorize(source(DemEncoding::Mapbox, TileFormat::JPG, &[0.0]), "dem_colorize").await?;
		assert_eq!(opaque.tilejson().tile_schema, Some(TileSchema::RasterRGB));
		assert_eq!(*opaque.metadata().tile_format(), TileFormat::JPG);
		let tile = opaque.tile(&TileCoord::new(3, 0, 0)?).await?.expect("a tile");
		assert!(matches!(tile.into_image()?, DynamicImage::ImageRgb8(_)));

		let translucent = colorize(
			source(DemEncoding::Mapbox, TileFormat::JPG, &[0.0]),
			"dem_colorize ramp=\"-1:2060a0,0:00000000\"",
		)
		.await?;
		assert_eq!(translucent.tilejson().tile_schema, Some(TileSchema::RasterRGBA));
		assert_eq!(*translucent.metadata().tile_format(), TileFormat::PNG);
		assert_eq!(pixel(&translucent, 0).await?, [0, 0, 0, 0]);
		Ok(())
	}

	#[tokio::test]
	async fn a_bad_ramp_is_refused_by_check_and_build_alike() {
		let factory = PipelineFactory::new_dummy();
		let vpl = "from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_colorize ramp=\"100:000000,0:FFFFFF\"";
		let error = factory.operation_from_vpl(vpl).await.unwrap_err();
		assert!(format!("{error:#}").contains("ascend"), "{error:#}");

		let problems = factory.check(&crate::vpl::parse_vpl(vpl).unwrap());
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(problems[0].message.contains("ascend"), "{problems:?}");
	}

	#[tokio::test]
	async fn a_source_that_is_not_mapbox_or_terrarium_is_refused() {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_debug format=png | dem_colorize",
			"from_debug format=png | meta_update schema=\"dem/versatiles\" | dem_colorize",
		] {
			let error = factory.operation_from_vpl(vpl).await.unwrap_err();
			assert!(format!("{error:#}").contains("encoding"), "{vpl}: {error:#}");
		}
	}

	/// The coloured relief the docs suggest: a hillshade laid over the colours
	/// with `from_stacked_raster`, which needs no blend operation of its own.
	#[tokio::test]
	async fn a_hillshade_stacks_over_the_colours() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let dem = "from_debug format=png | meta_update schema=\"dem/mapbox\"";
		let vpl = format!("from_stacked_raster [ {dem} | dem_hillshade, {dem} | dem_colorize ]");
		let op = factory.operation_from_vpl(&vpl).await?;
		let tile = op.tile(&TileCoord::new(3, 2, 1)?).await?.expect("a tile");
		let image = tile.into_image()?.to_rgba8();
		assert!(
			image.pixels().all(|p| p.0[3] == 255),
			"the colours below make it opaque"
		);
		Ok(())
	}
}
