use std::str;

use anyhow::{Result, ensure};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCompression, TileCoord, TileFormat, bounded_number};
use versatiles_derive::context;

use crate::{
	PipelineFactory,
	helpers::tile_format_subset::RasterTileFormat,
	operations::transform::{TileTransform, TransformOp},
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
/// to 13 use 70, zoom 14 uses 50, and zoom 15 and above use 20. `quality` is
/// ignored for PNG, which is always lossless.
///
/// `quality_translucent` is typically `100`: lossy encoders handle an alpha
/// channel badly. Setting it makes every tile that is encoded be checked for
/// opacity.
struct Args {
	/// Format to encode the tiles into. Defaults to the source's.
	format: Option<RasterTileFormat>,
	/// Encoder quality, `0` (worst) to `100` (lossless). Defaults to the encoder's own.
	quality: Option<QualityByZoom>,
	/// Encoder quality for tiles with translucent pixels. Defaults to using `quality` throughout.
	quality_translucent: Option<QualityByZoom>,
	/// Encoder effort, `0` is fastest and `100` smallest. Defaults to the encoder's own.
	effort: Option<Effort>,
	/// Whether to re-encode tiles that are already encoded in the target format. Defaults to `false`.
	#[vpl(default = "false")]
	force_reencode: Option<bool>,
}

#[derive(Debug)]
struct Operation {
	format: TileFormat,
	quality: [Option<u8>; 32],
	quality_translucent: Option<[Option<u8>; 32]>,
	effort: Option<u8>,
	force_reencode: bool,
}

impl Operation {
	#[context("Building raster_format operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		factory: &PipelineFactory,
	) -> Result<TransformOp<Operation>> {
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
		let operation = Operation {
			format: format.into(),
			quality: args.quality.unwrap_or_default().into_levels(),
			quality_translucent: args.quality_translucent.map(QualityByZoom::into_levels),
			effort: args.effort.map(Effort::get),
			force_reencode: args.force_reencode.unwrap_or(false),
		};
		Ok(TransformOp::new(source, operation, factory.runtime()))
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

		let level = coord.level as usize;
		let quality = self.quality[level];
		let effective_quality = match self.quality_translucent {
			Some(qt) if !tile.is_opaque()? => qt[level],
			_ => quality,
		};
		if is_encoded {
			// Forced: drop the encoded bytes, so that the tile is encoded again
			// even when no setting asks for it.
			tile.as_image_mut()?;
		}
		tile.change_format(self.format, effective_quality, self.effort)?;
		Ok(Some(tile))
	}
}

bounded_number! {
	/// How hard an encoder should work, from `0` (fastest) to `100` (smallest).
	///
	/// The same range `quality` enforces below, and for the same encoders — it
	/// was documented here and enforced nowhere until it had a type (#260).
	Effort: u8, 0..=100;
}

/// An encoder quality setting resolved per zoom level.
///
/// Carries the format of `quality=` in its type, so that `70,14:50,15:20` is
/// judged where a value is decoded rather than by whatever parses it later —
/// including by `check`, which never builds anything (#257).
///
/// `Default` is "nothing set at any zoom", which is what an absent `quality=`
/// means and what the encoder's own default fills in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QualityByZoom([Option<u8>; 32]);

impl QualityByZoom {
	/// The per-zoom levels, indexed by zoom.
	#[must_use]
	pub fn into_levels(self) -> [Option<u8>; 32] {
		self.0
	}
}

impl TryFrom<&str> for QualityByZoom {
	type Error = anyhow::Error;

	fn try_from(value: &str) -> Result<Self> {
		parse_quality(value).map(Self)
	}
}

#[context("Parsing quality string")]
fn parse_quality(text: &str) -> Result<[Option<u8>; 32]> {
	let mut result: [Option<u8>; 32] = [None; 32];
	let mut zoom: i32 = -1;
	for part in text.split(',') {
		let mut part = part.trim();
		zoom += 1;
		if part.is_empty() {
			continue;
		}
		if let Some(idx) = part.find(':') {
			zoom = part[0..idx].trim().parse()?;
			ensure!(
				(0..=31).contains(&zoom),
				"zoom level must be between 0 and 31, but is {zoom}"
			);
			part = &part[(idx + 1)..];
		}
		let quality_val: u8 = part.trim().parse()?;
		ensure!(
			quality_val <= 100,
			"quality must be between 0 and 100, but is {quality_val}"
		);
		for z in zoom..32 {
			result[usize::try_from(z).expect("zoom in 0..32 fits in usize")] = Some(quality_val);
		}
	}
	Ok(result)
}

crate::operations::macros::define_transform_factory!("raster_format", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use rstest::rstest;
	use versatiles_core::{TileCompression, TileCoord};

	use super::*;

	#[rstest]
	#[case("80 -> 80,80,80,80,80,80,80,80,80,80,80,80,80,80,80,80")]
	#[case("80,70 -> 80,70,70,70,70,70,70,70,70,70,70,70,70,70,70,70")]
	#[case("10:30 -> ,,,,,,,,,,30,30,30,30,30,30")]
	#[case("80,70,14:50,15:20 -> 80,70,70,70,70,70,70,70,70,70,70,70,70,70,50,20")]
	#[case(" -> ,,,,,,,,,,,,,,,")]
	#[case(", , -> ,,,,,,,,,,,,,,,")]
	#[case(" ,80 , ,  -> ,80,80,80,80,80,80,80,80,80,80,80,80,80,80,80")]
	fn parse_quality_cases(#[case] case: &str) -> Result<()> {
		let (input_str, expected_str) = case.split_once(" -> ").unwrap();
		let result = super::parse_quality(input_str)?;
		assert_eq!(result.len(), 32);
		let result_str = result[0..16]
			.iter()
			.map(|x| x.map_or(String::new(), |v| v.to_string()))
			.collect::<Vec<String>>()
			.join(",");

		assert_eq!(result_str, expected_str);
		Ok(())
	}

	#[rstest]
	#[case("32:10", "zoom level must be between 0 and 31, but is 32")] // invalid zoom
	#[case("5:101", "quality must be between 0 and 100, but is 101")] // invalid quality
	fn parse_quality_errors(#[case] input: &str, #[case] needle: &str) {
		let msg = super::parse_quality(input)
			.unwrap_err()
			.chain()
			.map(std::string::ToString::to_string)
			.collect::<Vec<_>>()
			.join("|");
		assert!(msg.contains(needle), "error '{msg}' should contain '{needle}'");
	}

	#[rstest]
	#[case("foo")]
	#[case("a:b")]
	#[case("5:x")]
	fn parse_quality_non_numeric_errors(#[case] input: &str) {
		assert!(super::parse_quality(input).is_err());
	}

	// --- which tiles the settings reach (#285) ---

	use versatiles_core::{Blob, TileFormat::*};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

	fn operation(format: TileFormat, quality: Option<u8>, effort: Option<u8>, force_reencode: bool) -> Operation {
		Operation {
			format,
			quality: [quality; 32],
			quality_translucent: None,
			effort,
			force_reencode,
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
		op.quality_translucent = Some([Some(100); 32]);

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
		let overscale = "from_debug format=webp | filter level_max=3 | raster_overscale";

		let upscaled = tile_of(overscale).await?.into_image()?;
		let blob = tile_of(&format!("{overscale} | raster_format effort=0"))
			.await?
			.into_blob(&TileCompression::Uncompressed)?;

		assert_eq!(blob, upscaled.to_blob(WEBP, None, Some(0))?);
		assert_ne!(blob, upscaled.to_blob(WEBP, None, None)?, "effort 0 should show");
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
