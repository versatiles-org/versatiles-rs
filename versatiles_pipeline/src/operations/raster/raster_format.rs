use std::{str, sync::Arc};

use anyhow::{Result, ensure};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCompression, TileCoord, TileFormat, bounded_number};
use versatiles_derive::context;
use versatiles_image::DynamicImageTraitConvert;

use crate::{
	PipelineFactory,
	helpers::{
		location::FileLocation,
		quality::{QualityByZoom, QualityMap, TileQuality},
		tile_format_subset::RasterTileFormat,
	},
	operations::transform::TileTransform,
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Sets the image format, quality and effort that raster tiles are encoded with.
///
/// The settings apply to every tile that has to be encoded anyway: one in
/// another format, and one that an earlier step of the pipeline created or
/// changed — upscaled, blended, resized, flattened. A tile that is already
/// encoded in the target format is passed through untouched, which costs no
/// time and, for a lossy format, no detail. So
/// `… | raster_overscale | raster_format quality=70 effort=0` encodes the
/// upscaled tiles quickly and leaves the tiles of the source as they are.
///
/// Set `force_reencode=true` to re-encode those tiles as well, e.g. to shrink
/// an existing tileset with a lower `quality`.
///
/// `quality` and `quality_translucent` take a zoom-dependent list as well as a
/// single number. In `quality="70,14:50,15:20"` the first value is the default
/// and each `zoom:value` pair applies from that zoom level upwards — so zoom 0
/// to 13 use 70, zoom 14 uses 50, and zoom 15 and above use 20. A value
/// without a zoom applies to the zoom after the previous one: `"70,50"` is 70
/// at zoom 0 and 50 from zoom 1 upwards. `quality` is ignored for PNG, which is
/// always lossless.
///
/// In place of a number, a zoom level can take a range that a quality map
/// chooses from: with `quality="70,16:30-50" quality_map="quality.png"`, zoom
/// 16 and above use 30 where the map is black, 50 where it is white, and an
/// interpolated quality in between. The map is an 8-bit grayscale PNG covering
/// the Web Mercator world, square, with a power of two as its side, so that a
/// pixel is a tile: in a map of 16384 pixels, a tile at zoom 14. A finer tile
/// takes the pixel it lies in. A coarser tile covers several pixels and takes
/// the highest quality any of them gives, so a tile with one white pixel in it
/// is encoded as if it were all white. A plain number ignores the map.
///
/// For elevation tiles (tile schema `dem/…`), `quality` defaults to `100`:
/// their channels are heights, and a lossy encoder would change them. `avif`
/// and `jpg` have no lossless encoding, so converting elevation tiles into one
/// of them needs a `quality` for every zoom level.
///
/// At a zoom level its list does not name, `quality_translucent` falls back
/// to `quality`. `quality_translucent` is typically `100`: lossy encoders handle an alpha
/// channel badly. Setting it makes every tile that is encoded be checked for
/// opacity.
///
/// With `lossless_if_smaller=true`, each tile that is encoded is encoded
/// twice, lossy with its `quality` and lossless, and the smaller of the two is
/// kept. Which one wins depends on the tile, not the zoom level: hillshading
/// of flat land, almost all of it faint alpha, comes out smaller lossless,
/// and that of high relief smaller lossy. In a sample of the planet's
/// hillshading at zoom 10 to 12, `quality=70 lossless_if_smaller=true` took
/// 59 % of the bytes of `quality=70` alone, and the tiles came out lossless
/// nine times out of ten. Encoding takes about 1.7 times as long. It needs
/// `format=webp`, the only format that has both a lossy and a lossless
/// encoding. A tile whose quality is `100` is encoded lossless only, once.
struct Args {
	/// Format to encode the tiles into. Defaults to the source's.
	format: Option<RasterTileFormat>,
	/// Encoder quality, `0` (worst) to `100` (lossless). Defaults to the encoder's own.
	quality: Option<QualityByZoom>,
	/// Encoder quality for tiles with translucent pixels. Defaults to using `quality` throughout.
	quality_translucent: Option<QualityByZoom>,
	/// Grayscale PNG that chooses a value within each quality range. Defaults to none.
	#[vpl(accepts = "png")]
	quality_map: Option<FileLocation>,
	/// Encoder effort, `0` is fastest and `100` smallest. Defaults to the encoder's own.
	effort: Option<Effort>,
	/// Whether to re-encode tiles that are already encoded in the target format. Defaults to `false`.
	#[vpl(default = "false")]
	force_reencode: Option<bool>,
	/// Whether to also encode each tile lossless and keep the smaller of the two. Defaults to `false`.
	#[vpl(default = "false")]
	lossless_if_smaller: Option<bool>,
}

#[derive(Debug)]
struct Operation {
	format: TileFormat,
	quality: TileQuality,
	quality_translucent: Option<TileQuality>,
	effort: Option<u8>,
	force_reencode: bool,
	lossless_if_smaller: bool,
	/// Whether the tiles hold heights, going by the tile schema of the source.
	elevation: bool,
}

impl Operation {
	#[context("Building raster_format operation in VPL node {:?}", vpl_node.name)]
	async fn build_transform(
		vpl_node: VPLNode,
		source: &dyn TileSource,
		factory: &PipelineFactory,
	) -> Result<Operation> {
		let args = Args::from_vpl_node(&vpl_node)?;

		// The one thing that has to happen here rather than in `update_metadata`:
		// with no `format=` the operation keeps the source's own format, which
		// means reading it before it is overwritten.
		let format: RasterTileFormat = match args.format {
			Some(f) => f,
			None => RasterTileFormat::try_from(*source.metadata().tile_format())?,
		};

		// Parsed already: `QualityByZoom` is what `Args` decodes into, so a bad
		// zoom list fails in `from_vpl_node` — and in `check`, which asks the
		// same parser without building anything.
		let quality = args.quality.unwrap_or_default();
		let uses_map = quality.uses_map() || args.quality_translucent.is_some_and(|qt| qt.uses_map());
		// A map nothing reads is a pipeline that does not do what it says.
		let map = match &args.quality_map {
			None => None,
			Some(location) => {
				ensure!(
					uses_map,
					"quality_map is read only by a quality range such as '16:30-50', \
					 and neither quality nor quality_translucent has one"
				);
				let png = location.read_all(factory).await?;
				let map =
					QualityMap::from_png(png.as_slice()).with_context(|| format!("reading the quality map {location}"))?;
				Some(Arc::new(map))
			}
		};
		let with_map = |levels: QualityByZoom| {
			TileQuality::new(levels, map.clone()).context("give the map with quality_map=\"….png\"")
		};

		let schema = source.tilejson().tile_schema.as_ref();
		let operation = Operation {
			format: format.into(),
			quality: with_map(quality)?,
			quality_translucent: args.quality_translucent.map(with_map).transpose()?,
			effort: args.effort.map(Effort::get),
			force_reencode: args.force_reencode.unwrap_or(false),
			lossless_if_smaller: args.lossless_if_smaller.unwrap_or(false),
			elevation: schema.is_some_and(versatiles_core::TileSchema::is_elevation),
		};

		// Ignoring the flag for another format would leave the user believing
		// the tiles were chosen per tile when they were not.
		ensure!(
			!operation.lossless_if_smaller || operation.format == TileFormat::WEBP,
			"lossless_if_smaller=true needs format=webp: '{}' {}",
			operation.format,
			if operation.format.can_be_lossless() {
				"is always lossless"
			} else {
				"has no lossless encoding"
			}
		);

		// A format without a lossless encoding cannot take quality 100, and
		// the encoder says so only at the first tile, from inside a stream.
		// Said here, it is an error of the pipeline and names the remedy.
		if !operation.format.can_be_lossless() {
			let asks_for_lossless = |quality: &TileQuality| quality.levels().can_give(100);
			ensure!(
				!asks_for_lossless(&operation.quality)
					&& !operation.quality_translucent.as_ref().is_some_and(asks_for_lossless),
				"'{}' has no lossless encoding: quality has to be below 100",
				operation.format
			);
			// Elevation tiles default to "without loss", which is not on offer
			// here. Only where the format changes: tiles already in the format
			// are passed on as they are.
			let changes_format = operation.format != *source.metadata().tile_format();
			ensure!(
				!(operation.elevation && changes_format && !operation.quality.levels().is_set_at_every_level()),
				"cannot encode elevation tiles ('{}') as '{}' without loss: '{}' has no lossless encoding. \
				 Use format=webp or format=png, or give a quality below 100 for every zoom level \
				 to accept that the heights change",
				schema.map_or_else(String::new, ToString::to_string),
				operation.format,
				operation.format
			);
		}
		Ok(operation)
	}
}

impl TileTransform for Operation {
	const TAG: &'static str = "raster_format";

	fn update_metadata(&self, metadata: &mut TileSourceMetadata) {
		metadata.set_tile_format(self.format);
		metadata.set_tile_compression(TileCompression::Uncompressed);
	}

	fn run(&self, coord: &TileCoord, mut tile: Tile) -> Result<Option<Tile>> {
		// A tile that is encoded already, and in the right format, is done. The
		// settings are for tiles that still have their encoding ahead of them
		// (#285): `quality` used to re-encode every tile here, so there was no
		// way to set it for the tiles a pipeline creates without also decoding
		// and re-encoding the ones it merely passes on.
		let is_encoded = tile.has_blob() && tile.format() == self.format;
		if is_encoded && !self.force_reencode {
			return Ok(Some(tile));
		}

		// Heights are not something to shave bits off by default: a lossy
		// encoder changes the low byte of a channel, which here is a height
		// off by hundreds of steps, or by far more where a byte carries over.
		// Whoever wants that has to ask for it with `quality`.
		let quality = self.quality.at(coord).or(self.elevation.then_some(100));
		// A zoom list for translucent tiles need not start at level 0. Below its
		// first entry there is no value, and "no value" has to mean "as for
		// any other tile": it used to mean the encoder's own default, whatever
		// `quality` said, and whatever the tiles held.
		let effective_quality = match &self.quality_translucent {
			Some(qt) if !tile.is_opaque()? => qt.at(coord).or(quality),
			_ => quality,
		};
		if self.lossless_if_smaller && effective_quality.is_none_or(|q| q < 100) {
			return self
				.smaller_of_lossy_and_lossless(&mut tile, effective_quality)
				.map(Some);
		}
		if is_encoded {
			// Forced: drop the encoded bytes, so that the tile is encoded again
			// even when no setting asks for it.
			tile.as_image_mut()?;
		}
		tile.change_format(self.format, effective_quality, self.effort)?;
		Ok(Some(tile))
	}
}

impl Operation {
	/// The tile encoded as WebP twice, lossy with `quality` and lossless,
	/// whichever is smaller; lossless where both are the same size, since it
	/// is exact. Encoded here rather than by the writer, which could encode it
	/// only one way.
	fn smaller_of_lossy_and_lossless(&self, tile: &mut Tile, quality: Option<u8>) -> Result<Tile> {
		let image = tile.as_image()?;
		let lossy = image.to_blob(TileFormat::WEBP, quality, self.effort)?;
		let lossless = image.to_blob(TileFormat::WEBP, Some(100), self.effort)?;
		let (blob, chosen) = if lossless.len() <= lossy.len() {
			(lossless, Some(100))
		} else {
			(lossy, quality)
		};
		let mut encoded = Tile::from_blob(blob, TileCompression::Uncompressed, TileFormat::WEBP);
		// What the tile is encoded with, for a later step that changes it and
		// has to encode it again.
		encoded.set_format_quality(chosen);
		encoded.set_format_effort(self.effort);
		Ok(encoded)
	}
}

bounded_number! {
	/// How hard an encoder should work, from `0` (fastest) to `100` (smallest).
	///
	/// The same range `quality` enforces, and for the same encoders — it
	/// was documented here and enforced nowhere until it had a type (#260).
	Effort: u8, 0..=100;
}

crate::operations::macros::define_transform_factory!("raster_format", Args, Operation, requires: Raster, per_tile);

#[cfg(test)]
mod tests {
	use rstest::rstest;
	// --- which tiles the settings reach (#285) ---
	use versatiles_core::{Blob, TileFormat::*};
	use versatiles_core::{TileCompression, TileCoord};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

	use super::*;

	/// One quality at every level, or none at any.
	fn constant(quality: Option<u8>) -> TileQuality {
		let text = quality.map_or_else(String::new, |q| q.to_string());
		TileQuality::new(QualityByZoom::try_from(text.as_str()).unwrap(), None).unwrap()
	}

	fn operation(format: TileFormat, quality: Option<u8>, effort: Option<u8>, force_reencode: bool) -> Operation {
		Operation {
			format,
			quality: constant(quality),
			quality_translucent: None,
			effort,
			force_reencode,
			lossless_if_smaller: false,
			elevation: false,
		}
	}

	/// Busy enough that quality and effort both show in the encoded size.
	#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
	fn image() -> DynamicImage {
		DynamicImage::from_fn(64, 64, |x, y| {
			[(x * 37 + y * 11) as u8, (x * y) as u8, ((x * 7) ^ (y * 13)) as u8]
		})
	}

	/// A tile as a container hands it out: encoded, not decoded.
	fn existing_tile(format: TileFormat) -> Result<(Tile, Blob)> {
		let blob = image().to_blob(format, None, None)?;
		Ok((
			Tile::from_blob(blob.clone(), TileCompression::Uncompressed, format),
			blob,
		))
	}

	/// A tile as an earlier step of a pipeline creates it: decoded, not encoded.
	fn new_tile(format: TileFormat) -> Result<Tile> {
		Tile::from_image(image(), format)
	}

	fn run(operation: &Operation, tile: Tile) -> Result<Blob> {
		let coord = TileCoord::new(3, 2, 2)?;
		operation
			.run(&coord, tile)?
			.unwrap()
			.into_blob(&TileCompression::Uncompressed)
	}

	#[rstest]
	#[case(None, Some(0))]
	#[case(Some(30), None)]
	#[case(Some(30), Some(0))]
	fn settings_reach_a_new_tile_and_leave_an_existing_one_alone(
		#[case] quality: Option<u8>,
		#[case] effort: Option<u8>,
	) -> Result<()> {
		let op = operation(WEBP, quality, effort, false);

		let (tile, original) = existing_tile(WEBP)?;
		assert_eq!(run(&op, tile)?, original, "an encoded tile must pass through untouched");

		let encoded = run(&op, new_tile(WEBP)?)?;
		assert_eq!(encoded, image().to_blob(WEBP, quality, effort)?);
		assert_ne!(encoded, original, "the settings should show in the result");
		Ok(())
	}

	#[rstest]
	#[case(None, Some(0))]
	#[case(Some(30), None)]
	#[case(None, None)]
	fn force_reencode_reaches_existing_tiles_too(#[case] quality: Option<u8>, #[case] effort: Option<u8>) -> Result<()> {
		let op = operation(WEBP, quality, effort, true);

		let (tile, original) = existing_tile(WEBP)?;
		// Encoded again from what the tile decodes to, not from the source image.
		let decoded = DynamicImage::from_blob(&original, WEBP)?;
		assert_eq!(run(&op, tile)?, decoded.to_blob(WEBP, quality, effort)?);

		assert_eq!(
			run(&op, new_tile(WEBP)?)?,
			image().to_blob(WEBP, quality, effort)?,
			"a new tile is encoded once, as without the flag"
		);
		Ok(())
	}

	#[test]
	fn a_tile_in_another_format_is_always_converted() -> Result<()> {
		let op = operation(WEBP, Some(30), Some(0), false);

		let (tile, original) = existing_tile(PNG)?;
		let decoded = DynamicImage::from_blob(&original, PNG)?;
		assert_eq!(run(&op, tile)?, decoded.to_blob(WEBP, Some(30), Some(0))?);
		Ok(())
	}

	#[test]
	fn quality_translucent_does_not_touch_an_existing_tile() -> Result<()> {
		let mut op = operation(WEBP, Some(30), None, false);
		op.quality_translucent = Some(constant(Some(100)));

		let (tile, original) = existing_tile(WEBP)?;
		let coord = TileCoord::new(3, 2, 2)?;
		let tile = op.run(&coord, tile)?.unwrap();
		assert!(!tile.has_content(), "an existing tile should not even be decoded");
		assert_eq!(tile.into_blob(&TileCompression::Uncompressed)?, original);
		Ok(())
	}

	/// The case from #285: an effort for the tiles `raster_overscale` creates.
	#[tokio::test]
	async fn an_effort_reaches_the_tiles_raster_overscale_creates() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let bbox = TileCoord::new(5, 9, 9)?.to_tile_bbox();
		let tile_of = async |vpl: &str| -> Result<Tile> {
			let op = factory.operation_from_vpl(vpl).await?;
			Ok(op.tile_stream(bbox).await?.to_vec().await.remove(0).1)
		};
		let overscale = "from_debug format=webp | filter level_max=3 | raster_overscale level_max=5";

		let upscaled = tile_of(overscale).await?.into_image()?;
		let blob = tile_of(&format!("{overscale} | raster_format effort=0"))
			.await?
			.into_blob(&TileCompression::Uncompressed)?;

		assert_eq!(blob, upscaled.to_blob(WEBP, None, Some(0))?);
		assert_ne!(blob, upscaled.to_blob(WEBP, None, None)?, "effort 0 should show");
		Ok(())
	}

	/// Changing the format of elevation tiles without saying anything about
	/// quality must not cost heights; asking for a quality still does.
	#[tokio::test]
	async fn elevation_tiles_are_lossless_unless_a_quality_is_asked_for() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let heights_survive = async |format_args: &str| -> Result<bool> {
			let vpl = format!("from_debug format=png | meta_update schema=\"dem/mapbox\" | raster_format {format_args}");
			let op = factory.operation_from_vpl(&vpl).await?;
			let tile = op.tile_stream(bbox).await?.to_vec().await.remove(0).1;
			let image = tile.clone().into_image()?.to_rgb8();
			let blob = tile.into_blob(&TileCompression::Uncompressed)?;
			Ok(DynamicImage::from_blob(&blob, WEBP)?.to_rgb8().as_raw() == image.as_raw())
		};

		assert!(heights_survive("format=webp").await?);
		assert!(heights_survive("format=webp effort=0").await?);
		assert!(!heights_survive("format=webp quality=80").await?);
		Ok(())
	}

	/// AVIF and JPEG have no lossless encoding, so elevation tiles cannot get
	/// their default there. That used to surface at the first tile, as the
	/// encoder's refusal from inside a stream.
	#[rstest]
	#[case("format=avif", Some("without loss"))]
	#[case("format=jpg", Some("without loss"))]
	#[case("format=avif quality=\"14:80\"", Some("for every zoom level"))]
	#[case("format=avif quality=100", Some("has no lossless encoding"))]
	#[case("format=avif quality=90", None)]
	#[case("format=jpg quality=\"90,14:80\"", None)]
	#[case("format=webp", None)]
	#[case("", None)]
	#[tokio::test]
	async fn elevation_tiles_into_a_format_without_lossless_need_a_quality(
		#[case] format_args: &str,
		#[case] refused_with: Option<&str>,
	) {
		let factory = PipelineFactory::new_dummy();
		let vpl = format!("from_debug format=png | meta_update schema=\"dem/mapbox\" | raster_format {format_args}");
		let result = factory.operation_from_vpl(&vpl).await;
		match refused_with {
			Some(needle) => {
				let Err(error) = result else {
					panic!("should be refused: {vpl}");
				};
				let message = format!("{error:#}");
				assert!(message.contains(needle), "{message}");
			}
			None => assert!(result.is_ok(), "{:#}", result.err().unwrap()),
		}

		// For a source that is not elevation data, the encoder's default is fine.
		let plain = format!("from_debug format=png | raster_format {format_args}");
		let refused = factory.operation_from_vpl(&plain).await.is_err();
		assert_eq!(refused, format_args.contains("quality=100"), "{plain}");
	}

	/// `quality_translucent="10:90"` says nothing about the levels below 10.
	/// There a translucent tile gets what every other tile gets: `quality`,
	/// or for elevation tiles the lossless default. It used to get the
	/// encoder's own default, which made elevation tiles lossy.
	#[rstest]
	#[case::below_the_list(Some(40), false, 3, Some(40))]
	#[case::inside_the_list(Some(40), false, 12, Some(90))]
	#[case::no_quality_at_all(None, false, 3, None)]
	#[case::elevation_below_the_list(None, true, 3, Some(100))]
	#[case::elevation_inside_the_list(None, true, 12, Some(90))]
	fn quality_translucent_falls_back_to_quality_below_its_first_level(
		#[case] quality: Option<u8>,
		#[case] elevation: bool,
		#[case] level: u8,
		#[case] expected: Option<u8>,
	) -> Result<()> {
		let op = Operation {
			quality_translucent: Some(TileQuality::new(QualityByZoom::try_from("10:90")?, None)?),
			elevation,
			..operation(WEBP, quality, None, false)
		};

		#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
		let image = DynamicImage::from_fn(16, 16, |x, y| [x as u8, y as u8, 7u8, if x < 8 { 255 } else { 100 }]);
		let tile = op
			.run(&TileCoord::new(level, 0, 0)?, Tile::from_image(image, WEBP)?)?
			.unwrap();
		assert_eq!(tile.format_quality(), expected);
		Ok(())
	}

	// --- quality_map ---

	/// A 2×2 map, a pixel per tile at zoom 1: black in the west, white in
	/// the east. Returns the directory, which lives as long as it, and the
	/// map's path in single quotes, which keep a Windows path's backslashes.
	fn write_map() -> Result<(assert_fs::TempDir, String)> {
		use imageproc::image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
		let dir = assert_fs::TempDir::new()?;
		let mut png = Vec::new();
		PngEncoder::new(&mut png).write_image(&[0, 255, 0, 255], 2, 2, ExtendedColorType::L8)?;
		let path = dir.path().join("quality.png");
		std::fs::write(&path, png)?;
		let quoted = format!("'{}'", path.display());
		Ok((dir, quoted))
	}

	/// The quality each tile of zoom 0 to 2 in the north-west and the
	/// north-east is encoded with.
	#[tokio::test]
	async fn a_quality_range_follows_the_map() -> Result<()> {
		let (_dir, map) = write_map()?;
		let op = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!(
				"from_debug format=png | raster_format format=webp quality=\"20,1:30-50,2:60\" quality_map={map}"
			))
			.await?;
		let quality_of = async |level: u8, x: u32| -> Result<Option<u8>> {
			let bbox = TileCoord::new(level, x, 0)?.to_tile_bbox();
			Ok(op.tile_stream(bbox).await?.to_vec().await.remove(0).1.format_quality())
		};
		assert_eq!(quality_of(0, 0).await?, Some(20), "a number ignores the map");
		assert_eq!(quality_of(1, 0).await?, Some(30), "black");
		assert_eq!(quality_of(1, 1).await?, Some(50), "white");
		assert_eq!(quality_of(2, 0).await?, Some(60));
		Ok(())
	}

	#[rstest]
	#[case("quality=\"70,1:30-50\"", "needs a quality map")]
	#[case("quality=70 quality_map=MAP", "read only by a quality range")]
	#[case("quality=\"30-50\" quality_map=MAP", "")]
	#[case("quality=\"30-50\" quality_map='missing.png'", "missing.png")]
	#[tokio::test]
	async fn a_range_and_a_map_need_each_other(#[case] args: &str, #[case] needle: &str) -> Result<()> {
		let (_dir, map) = write_map()?;
		let vpl = format!(
			"from_debug format=png | raster_format format=webp {}",
			args.replace("MAP", &map)
		);
		let result = PipelineFactory::new_dummy().operation_from_vpl(&vpl).await;
		if needle.is_empty() {
			result?;
			return Ok(());
		}
		let Err(error) = result else {
			panic!("should be refused: {vpl}");
		};
		let message = format!("{error:#}");
		assert!(message.contains(needle), "{message}");
		Ok(())
	}

	// --- lossless_if_smaller ---

	/// Hillshading of flat land in miniature: a faint alpha over a grey that
	/// varies, which lossless WebP encodes in far fewer bytes than lossy.
	#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
	fn flat_image() -> DynamicImage {
		DynamicImage::from_fn(64, 64, |x, y| {
			let grey = (x * 4) as u8;
			[grey, grey, grey, if (x + y) % 2 == 0 { 3 } else { 4 }]
		})
	}

	/// Noise, which lossless WebP cannot shrink and lossy WebP can.
	fn noisy_image() -> DynamicImage {
		let mut state = 1u32;
		let mut next = move || {
			state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
			state.to_be_bytes()[1]
		};
		DynamicImage::from_fn(64, 64, |_, _| [next(), next(), next()])
	}

	fn adaptive(quality: Option<u8>, force_reencode: bool) -> Operation {
		Operation {
			lossless_if_smaller: true,
			..operation(WEBP, quality, None, force_reencode)
		}
	}

	#[rstest]
	#[case::flat_land_is_lossless(flat_image(), true)]
	#[case::noise_is_lossy(noisy_image(), false)]
	fn lossless_if_smaller_keeps_the_smaller_encoding(
		#[case] image: DynamicImage,
		#[case] lossless_wins: bool,
	) -> Result<()> {
		let lossy = image.to_blob(WEBP, Some(70), None)?;
		let lossless = image.to_blob(WEBP, Some(100), None)?;
		assert_eq!(
			lossless.len() < lossy.len(),
			lossless_wins,
			"the test image should favour one"
		);

		let coord = TileCoord::new(3, 2, 2)?;
		let tile = adaptive(Some(70), false)
			.run(&coord, Tile::from_image(image, PNG)?)?
			.unwrap();
		assert!(tile.has_blob(), "the tile should leave encoded");
		assert_eq!(tile.format(), WEBP);
		assert_eq!(tile.format_quality(), Some(if lossless_wins { 100 } else { 70 }));
		let blob = tile.into_blob(&TileCompression::Uncompressed)?;
		assert_eq!(blob, if lossless_wins { lossless } else { lossy });
		Ok(())
	}

	#[test]
	fn lossless_if_smaller_leaves_an_existing_tile_alone_unless_forced() -> Result<()> {
		let original = flat_image().to_blob(WEBP, Some(70), None)?;
		let existing = || Tile::from_blob(original.clone(), TileCompression::Uncompressed, WEBP);

		assert_eq!(run(&adaptive(Some(70), false), existing())?, original);

		let decoded = DynamicImage::from_blob(&original, WEBP)?;
		assert_eq!(
			run(&adaptive(Some(70), true), existing())?,
			decoded.to_blob(WEBP, Some(100), None)?,
			"forced, the tile should be compared as well"
		);
		Ok(())
	}

	#[test]
	fn lossless_if_smaller_with_quality_100_is_lossless() -> Result<()> {
		assert_eq!(
			run(&adaptive(Some(100), false), Tile::from_image(noisy_image(), WEBP)?)?,
			noisy_image().to_blob(WEBP, Some(100), None)?
		);
		Ok(())
	}

	#[rstest]
	#[case("format=png", Some("'png' is always lossless"))]
	#[case("", Some("'png' is always lossless"))]
	#[case("format=avif quality=50", Some("'avif' has no lossless encoding"))]
	#[case("format=jpg quality=50", Some("'jpg' has no lossless encoding"))]
	#[case("format=webp quality=70", None)]
	#[tokio::test]
	async fn lossless_if_smaller_needs_webp(
		#[case] format_args: &str,
		#[case] refused_with: Option<&str>,
	) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let vpl = format!("from_debug format=png | raster_format {format_args} lossless_if_smaller=true");
		match (factory.operation_from_vpl(&vpl).await, refused_with) {
			(Err(error), Some(needle)) => {
				let message = format!("{error:#}");
				assert!(message.contains(needle), "{message}");
				assert!(message.contains("needs format=webp"), "{message}");
			}
			(Ok(op), None) => {
				let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
				let (_, tile) = op.tile_stream(bbox).await?.to_vec().await.remove(0);
				assert!(tile.has_blob(), "the tile should leave encoded");
			}
			(Err(error), None) => panic!("should be accepted: {vpl}: {error:#}"),
			(Ok(_), Some(_)) => panic!("should be refused: {vpl}"),
		}
		Ok(())
	}

	#[tokio::test]
	async fn force_reencode_is_a_vpl_parameter() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		factory
			.operation_from_vpl("from_debug format=png | raster_format quality=80 force_reencode=true")
			.await?;
		Ok(())
	}

	#[tokio::test]
	async fn test_raster_format() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_format format=webp quality=80 effort=40")
			.await?;

		// Parameters must reflect the target format and uncompressed tile_compression
		let params = op.metadata().clone();
		assert_eq!(*params.tile_format(), TileFormat::WEBP);
		assert_eq!(*params.tile_compression(), TileCompression::Uncompressed);

		// Stream should still yield exactly one tile and the tile should be WEBP now
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let mut items = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(items.len(), 1, "expected exactly one tile at z=3, x=2, y=2");
		let (_coord, tile) = items.remove(0);
		assert_eq!(tile.format(), TileFormat::WEBP);
		Ok(())
	}

	#[tokio::test]
	async fn test_raster_format_with_quality_translucent() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// Test that quality_translucent parameter is accepted and the pipeline builds successfully
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_format format=webp quality=80 quality_translucent=100")
			.await?;

		let params = op.metadata().clone();
		assert_eq!(*params.tile_format(), TileFormat::WEBP);
		assert_eq!(*params.tile_compression(), TileCompression::Uncompressed);

		// Stream should yield tiles
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let mut items = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(items.len(), 1);
		let (_coord, tile) = items.remove(0);
		assert_eq!(tile.format(), TileFormat::WEBP);
		Ok(())
	}
}
