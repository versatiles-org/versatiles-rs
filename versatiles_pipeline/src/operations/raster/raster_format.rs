use crate::{PipelineFactory, traits::*, vpl::VPLNode};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use std::{fmt::Debug, str, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSourceTrait};
use versatiles_core::*;
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Filter tiles by bounding box and/or zoom levels.
struct Args {
	/// The desired tile format. Allowed values are: AVIF, JPG, PNG or WEBP.
	/// If not specified, the source format will be used.
	format: Option<String>,
	/// Quality level for the tile compression (only AVIF, JPG or WEBP), between 0 (worst) and 100 (lossless).
	/// To allow different quality levels for different zoom levels, this can also be a comma-separated list like this:
	/// "80,70,14:50,15:20", where the first value is the default quality, and the other values specify the quality for the specified zoom level (and higher).
	quality: Option<String>,
	/// Compression speed (only AVIF), between 0 (slowest) and 100 (fastest).
	speed: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RasterTileFormat {
	Avif,
	Jpeg,
	Png,
	Webp,
}

impl RasterTileFormat {
	#[context("Parsing raster tile format from string '{text}'")]
	fn from_str(text: &str) -> Result<Self> {
		use RasterTileFormat::*;
		Ok(match text.to_lowercase().trim() {
			"avif" => Avif,
			"jpg" | "jpeg" => Jpeg,
			"png" => Png,
			"webp" => Webp,
			_ => bail!("Invalid tile format '{text}'"),
		})
	}
}

impl TryFrom<TileFormat> for RasterTileFormat {
	type Error = anyhow::Error;
	fn try_from(value: TileFormat) -> std::result::Result<Self, Self::Error> {
		use RasterTileFormat::*;
		Ok(match value {
			TileFormat::AVIF => Avif,
			TileFormat::JPG => Jpeg,
			TileFormat::PNG => Png,
			TileFormat::WEBP => Webp,
			_ => bail!("Invalid tile format '{value}' for raster operations"),
		})
	}
}

impl From<RasterTileFormat> for TileFormat {
	fn from(value: RasterTileFormat) -> Self {
		use RasterTileFormat::*;
		match value {
			Avif => TileFormat::AVIF,
			Jpeg => TileFormat::JPG,
			Png => TileFormat::PNG,
			Webp => TileFormat::WEBP,
		}
	}
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	source: Box<dyn TileSourceTrait>,
	tilejson: TileJSON,
	format: RasterTileFormat,
	quality: [Option<u8>; 32],
	speed: Option<u8>,
}

impl Operation {
	#[context("Building raster_format operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSourceTrait>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSourceTrait,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let mut parameters = source.parameters().clone();

		let format: RasterTileFormat = if let Some(text) = args.format {
			RasterTileFormat::from_str(&text)?
		} else {
			RasterTileFormat::try_from(parameters.tile_format)?
		};

		parameters.tile_format = format.into();
		parameters.tile_compression = TileCompression::Uncompressed;

		let mut tilejson = source.tilejson().clone();
		tilejson.update_from_reader_parameters(&parameters);

		Ok(Self {
			format,
			quality: parse_quality(args.quality)?,
			speed: args.speed,
			parameters,
			source,
			tilejson,
		})
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
				ensure!(zoom <= 31, "Zoom level must be between 0 and 31");
				part = &part[(idx + 1)..];
			}
			let quality_val: u8 = part.trim().parse()?;
			ensure!(quality_val <= 100, "Quality value must be between 0 and 100");
			for z in zoom..32 {
				result[z as usize] = Some(quality_val);
			}
		}
	}
	Ok(result)
}

#[async_trait]
impl TileSourceTrait for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_format", self.source.source_type())
	}

	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		self.source.traversal()
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_tile_stream {:?}", bbox);

		let quality = self.quality[bbox.level as usize];
		let speed = self.speed;
		let stream = self.source.get_tile_stream(bbox).await?;
		let format: TileFormat = self.format.into();

		Ok(stream.map_item_parallel(move |mut tile| {
			tile.change_format(format, quality, speed)?;
			Ok(tile)
		}))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"raster_format"
	}
}

#[async_trait]
impl TransformOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		source: Box<dyn TileSourceTrait>,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn TileSourceTrait>> {
		Operation::build(vpl_node, source, factory)
			.await
			.map(|op| Box::new(op) as Box<dyn TileSourceTrait>)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

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
			.map(|x| x.map(|v| v.to_string()).unwrap_or(String::new()))
			.collect::<Vec<String>>()
			.join(",");

		assert_eq!(result_str, expected_str);
		Ok(())
	}

	#[rstest]
	#[case("32:10", "Zoom level must be between 0 and 31")] // invalid zoom
	#[case("5:101", "Quality value must be between 0 and 100")] // invalid quality
	fn parse_quality_errors(#[case] input: &str, #[case] needle: &str) {
		let msg = super::parse_quality(Some(input.to_string()))
			.unwrap_err()
			.chain()
			.map(|e| e.to_string())
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

	#[rstest]
	#[case("avif", RasterTileFormat::Avif)]
	#[case("jpg", RasterTileFormat::Jpeg)]
	#[case("jpeg", RasterTileFormat::Jpeg)]
	#[case("png", RasterTileFormat::Png)]
	#[case("webp", RasterTileFormat::Webp)]
	fn raster_tile_format_from_str_ok(#[case] s: &str, #[case] expected: RasterTileFormat) {
		assert_eq!(RasterTileFormat::from_str(s).unwrap(), expected);
	}

	#[test]
	fn raster_tile_format_from_str_err() {
		assert!(RasterTileFormat::from_str("tiff").is_err());
	}

	#[rstest]
	#[case(TileFormat::AVIF, RasterTileFormat::Avif)]
	#[case(TileFormat::JPG, RasterTileFormat::Jpeg)]
	#[case(TileFormat::PNG, RasterTileFormat::Png)]
	#[case(TileFormat::WEBP, RasterTileFormat::Webp)]
	fn raster_tile_format_try_from_tileformat(#[case] input: TileFormat, #[case] expected: RasterTileFormat) {
		assert_eq!(RasterTileFormat::try_from(input).unwrap(), expected);
	}

	#[tokio::test]
	async fn test_raster_format() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | raster_format format=webp quality=80 speed=60")
			.await?;

		// Parameters must reflect the target format and uncompressed tile_compression
		let params = op.parameters().clone();
		assert_eq!(params.tile_format, TileFormat::WEBP);
		assert_eq!(params.tile_compression, TileCompression::Uncompressed);

		// Stream should still yield exactly one tile and the tile should be WEBP now
		let bbox = TileCoord::new(3, 2, 2)?.to_tile_bbox();
		let mut items = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(items.len(), 1, "expected exactly one tile at z=3, x=2, y=2");
		let (_coord, tile) = items.remove(0);
		assert_eq!(tile.format(), TileFormat::WEBP);
		Ok(())
	}
}
