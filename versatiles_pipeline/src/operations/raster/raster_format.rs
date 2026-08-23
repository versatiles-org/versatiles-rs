use std::str;

use anyhow::{Result, ensure};
use versatiles_container::{Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileCompression, TileCoord, TileFormat};
use versatiles_derive::context;

use crate::{
	PipelineFactory,
	helpers::tile_format_subset::RasterTileFormat,
	operations::transform::{TileTransform, TransformOp},
	vpl::VPLNode,
};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Re-encodes raster tiles into another image format, quality or effort setting.
///
/// `quality` and `quality_translucent` take a zoom-dependent list as well as a
/// single number. In `quality="70,14:50,15:20"` the first value is the default
/// and each `zoom:value` pair applies from that zoom level upwards — so zoom 0
/// to 13 use 70, zoom 14 uses 50, and zoom 15 and above use 20. Tiles that are
/// already in the target format and need no quality change are passed through
/// without re-encoding. `quality` is ignored for PNG, which is always lossless.
///
/// `quality_translucent` is typically `100`: lossy encoders handle an alpha
/// channel badly. Setting it makes every tile be checked for opacity.
struct Args {
	/// Format to encode the tiles into. Defaults to the source's.
	format: Option<RasterTileFormat>,
	/// Encoder quality, `0` (worst) to `100` (lossless). Defaults to the encoder's own.
	quality: Option<String>,
	/// Encoder quality for tiles with translucent pixels. Defaults to using `quality` throughout.
	quality_translucent: Option<String>,
	/// Encoder effort, `0` (fastest) to `100` (smallest). Defaults to the encoder's own.
	effort: Option<u8>,
}

#[derive(Debug)]
struct Operation {
	format: TileFormat,
	quality: [Option<u8>; 32],
	quality_translucent: Option<[Option<u8>; 32]>,
	effort: Option<u8>,
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

		let quality_translucent = if let Some(qt) = args.quality_translucent {
			Some(parse_quality(Some(qt))?)
		} else {
			None
		};

		let operation = Operation {
			format: format.into(),
			quality: parse_quality(args.quality)?,
			quality_translucent,
			effort: args.effort,
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
		let level = coord.level as usize;
		let quality = self.quality[level];
		let effective_quality = match self.quality_translucent {
			Some(qt) if !tile.is_opaque()? => qt[level],
			_ => quality,
		};
		tile.change_format(self.format, effective_quality, self.effort)?;
		Ok(Some(tile))
	}
}

#[context("Parsing quality string")]
fn parse_quality(quality: Option<String>) -> Result<[Option<u8>; 32]> {
	let mut result: [Option<u8>; 32] = [None; 32];
	if let Some(text) = quality {
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
		let result = super::parse_quality(Some(input_str.to_string()))?;
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
		let msg = super::parse_quality(Some(input.to_string()))
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
		assert!(super::parse_quality(Some(input.to_string())).is_err());
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
