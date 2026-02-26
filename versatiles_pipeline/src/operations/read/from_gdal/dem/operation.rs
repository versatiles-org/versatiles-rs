use super::{DemSource, dem_source::DemEncoding};
use crate::{
	PipelineFactory,
	factory::{OperationFactoryTrait, ReadOperationFactoryTrait},
	operations::read::traits::ReadTileSource,
	vpl::VPLNode,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileSchema, TileStream};
use versatiles_derive::context;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Reads a GDAL DEM dataset and produces terrain RGB tiles (Mapbox or Terrarium encoding).
struct Args {
	/// The filename of the GDAL DEM dataset to read.
	/// For example: `filename="dem.tif"`.
	filename: String,
	/// The DEM encoding format: `"mapbox"` or `"terrarium"`. (default: `"mapbox"`)
	encoding: Option<String>,
	/// The size of the generated tiles in pixels. (default: 512)
	tile_size: Option<u32>,
	/// The maximum zoom level to generate tiles for.
	/// (default: the maximum zoom level based on the dataset's native resolution)
	level_max: Option<u8>,
	/// The minimum zoom level to generate tiles for. (default: level_max)
	level_min: Option<u8>,
	/// How often to reuse a GDAL instance. (default: 100)
	/// Set to a lower value if you have problems like memory leaks in GDAL.
	gdal_reuse_limit: Option<u32>,
	/// The number of maximum concurrent GDAL instances to allow. (default: 4)
	/// Set to a higher value if you have enough system resources and want to increase throughput.
	gdal_concurrency_limit: Option<u8>,
}

#[derive(Debug)]
struct Operation {
	source: Arc<DemSource>,
	metadata: TileSourceMetadata,
	tilejson: TileJSON,
	tile_size: u32,
	encoding: DemEncoding,
}

impl Operation {
	#[context("Building from_gdal_dem operation in VPL node {:?}", vpl_node.name)]
	async fn new(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Self>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node).context("Failed to parse arguments from VPL node")?;

		let encoding = match args.encoding.as_deref() {
			None | Some("mapbox") => DemEncoding::Mapbox,
			Some("terrarium") => DemEncoding::Terrarium,
			Some(other) => bail!("Unknown DEM encoding: \"{other}\". Expected \"mapbox\" or \"terrarium\"."),
		};

		let filename = factory.resolve_path(&args.filename);
		let source = DemSource::new(
			&filename,
			args.gdal_reuse_limit.unwrap_or(100),
			args.gdal_concurrency_limit.unwrap_or(4) as usize,
		)
		.await?;
		let bbox = source.bbox();
		let tile_size = args.tile_size.unwrap_or(512);

		let level_max = args.level_max.unwrap_or(source.level_max(tile_size)?);
		let level_min = args.level_min.unwrap_or(level_max);
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(level_min, level_max, bbox);

		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			bbox_pyramid,
			Traversal::ANY,
		);

		let tile_schema = match encoding {
			DemEncoding::Mapbox => TileSchema::RasterDEMMapbox,
			DemEncoding::Terrarium => TileSchema::RasterDEMTerrarium,
		};

		let mut tilejson = TileJSON {
			bounds: Some(*bbox),
			..Default::default()
		};
		metadata.update_tilejson(&mut tilejson);
		tilejson.tile_schema = Some(tile_schema);

		Ok(Self {
			source: Arc::new(source),
			metadata,
			tilejson,
			tile_size,
			encoding,
		})
	}
}

impl ReadTileSource for Operation {
	#[context("Failed to build read operation")]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		Ok(Box::new(Self::new(vpl_node, factory).await?) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_container("gdal_dem", "gdal_dem")
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, mut bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		let count = 8192u32.div_euclid(self.tile_size).max(1);

		bbox.intersect_with_pyramid(&self.metadata.bbox_pyramid);

		let bboxes: Vec<TileBBox> = bbox.iter_bbox_grid(count).collect();
		let size = self.tile_size;
		let tile_format = self.metadata.tile_format;
		let source = Arc::clone(&self.source);
		let encoding = self.encoding;

		use futures::stream::{self, StreamExt};
		let streams = stream::iter(bboxes).map(move |bbox| {
			let source = Arc::clone(&source);
			async move {
				if bbox.is_empty() {
					return TileStream::empty();
				}

				let geo_bbox = bbox.to_geo_bbox().unwrap();
				let width = (size * bbox.width()) as usize;
				let height = (size * bbox.height()) as usize;

				let image = source
					.get_elevation_tile(&geo_bbox, width, height, encoding)
					.await
					.unwrap();

				if let Some(image) = image {
					let vec = tokio::task::spawn_blocking(move || {
						bbox
							.iter_coords_zorder()
							.map(|coord| {
								let tile_img = image.crop_imm(
									(coord.x - bbox.x_min().unwrap()) * size,
									(coord.y - bbox.y_min().unwrap()) * size,
									size,
									size,
								);
								(coord, Tile::from_image(tile_img, tile_format).unwrap())
							})
							.collect::<Vec<_>>()
					})
					.await
					.unwrap();

					TileStream::from_vec(vec)
				} else {
					TileStream::empty()
				}
			}
		});

		Ok(TileStream::from_streams(streams))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_gdal_dem"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSource>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "from_gdal_dem");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("filename"));
		assert!(docs.contains("encoding"));
		assert!(docs.contains("tile_size"));
		assert!(docs.contains("level_max"));
		assert!(docs.contains("level_min"));
	}
}
