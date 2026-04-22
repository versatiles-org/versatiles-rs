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
use versatiles_container::{DataLocation, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileCompression, TileFormat, TileJSON, TilePyramid, TileSchema, TileStream};
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
	/// Optional path to a GeoJSON file with Polygon/MultiPolygon geometry.
	/// Only pixels inside the polygon will be rendered; everything outside becomes nodata.
	cutline: Option<String>,
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

		let filename = factory
			.resolve_location(&DataLocation::try_from(&args.filename)?)?
			.to_path_buf()?;

		let cutline_path = args
			.cutline
			.as_ref()
			.map(|c| {
				factory
					.resolve_location(&DataLocation::try_from(c.as_str())?)
					.and_then(|l| l.to_path_buf())
			})
			.transpose()?;

		let source = DemSource::new(
			&filename,
			args.gdal_reuse_limit.unwrap_or(100),
			args.gdal_concurrency_limit.unwrap_or(4) as usize,
			cutline_path.as_deref(),
		)
		.await?;
		let mut bbox = *source.bbox();
		if let Some(cutline_bbox) = source.cutline_bbox() {
			bbox.intersect(cutline_bbox);
		}
		let bbox = &bbox;
		let tile_size = args.tile_size.unwrap_or(512);

		let level_max = args.level_max.unwrap_or(source.level_max(tile_size)?);
		let level_min = args.level_min.unwrap_or(level_max);
		let tile_pyramid = TilePyramid::from_geo_bbox(level_min, level_max, bbox)?;

		let metadata = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(tile_pyramid),
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
		tilejson.set_tile_size(tile_size)?;

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

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set"))
	}

	#[context("Failed to get stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_gdal_dem::tile_stream {bbox:?}");
		let count = 8192u32.div_euclid(self.tile_size).max(1);

		let bbox = self.metadata.intersection_bbox(&bbox);

		let bboxes: Vec<TileBBox> = bbox.iter_grid(count).collect();
		let size = self.tile_size;
		let tile_format = *self.metadata.tile_format();
		let source = Arc::clone(&self.source);
		let encoding = self.encoding;

		use futures::stream::{self, StreamExt};
		let streams = stream::iter(bboxes).map(move |bbox| {
			let source = Arc::clone(&source);
			async move {
				if bbox.is_empty() {
					return TileStream::empty();
				}

				let geo_bbox = bbox.to_geo_bbox().expect("bbox is non-empty");
				let width = (size * bbox.width()) as usize;
				let height = (size * bbox.height()) as usize;

				let image = source
					.elevation_tile(&geo_bbox, width, height, encoding)
					.await
					.expect("elevation_tile succeeded for bbox");

				if let Some(image) = image {
					let vec = tokio::task::spawn_blocking(move || {
						bbox
							.iter_coords_zorder()
							.map(|coord| {
								let tile_img = image.crop_imm(
									(coord.x - bbox.x_min().expect("bbox is non-empty")) * size,
									(coord.y - bbox.y_min().expect("bbox is non-empty")) * size,
									size,
									size,
								);
								(
									coord,
									Tile::from_image(tile_img, tile_format).expect("tile_format is raster"),
								)
							})
							.collect::<Vec<_>>()
					})
					.await
					.expect("spawn_blocking task did not panic");

					TileStream::from_vec(vec)
				} else {
					TileStream::empty()
				}
			}
		});

		Ok(TileStream::from_streams(streams))
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let bbox = self.metadata.intersection_bbox(&bbox);
		Ok(TileStream::from_iter_coord(bbox.into_iter_coords(), move |_coord| {
			Some(())
		}))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn docs(&self) -> String {
		Args::docs()
	}
	fn tag_name(&self) -> &str {
		"from_gdal_dem"
	}
	#[cfg(feature = "codegen")]
	fn field_metadata(&self) -> Vec<crate::vpl::VPLFieldMeta> {
		Args::field_metadata()
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
	use assert_fs::TempDir;
	use gdal::DriverManager;
	use rstest::rstest;
	use std::path::Path;
	use versatiles_core::GeoBBox;

	/// Creates a temporary single-band float32 GeoTIFF with a gradient of elevation values.
	fn create_test_dem(path: &Path, bbox: &GeoBBox) {
		let size = 256;
		let driver = DriverManager::get_driver_by_name("GTiff").unwrap();
		let mut ds = driver
			.create_with_band_type::<f32, _>(path.to_str().unwrap(), size, size, 1)
			.unwrap();
		ds.set_spatial_ref(&super::super::get_spatial_ref(4326).unwrap())
			.unwrap();
		ds.set_geo_transform(&[
			bbox.x_min,
			(bbox.x_max - bbox.x_min) / size as f64,
			0.0,
			bbox.y_max,
			0.0,
			(bbox.y_min - bbox.y_max) / size as f64,
		])
		.unwrap();

		let mut elev_data = vec![0.0f32; size * size];
		for row in 0..size {
			for col in 0..size {
				elev_data[row * size + col] = (col as f32 / size as f32) * 8848.0;
			}
		}
		let mut buffer = gdal::raster::Buffer::new((size, size), elev_data);
		ds.rasterband(1)
			.unwrap()
			.write((0, 0), (size, size), &mut buffer)
			.unwrap();
	}

	fn create_temp_dem() -> (TempDir, String) {
		let tmp = TempDir::new().unwrap();
		let dem_path = tmp.path().join("test_dem.tif");
		let bbox = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		create_test_dem(&dem_path, &bbox);
		let path_str = dem_path.to_str().unwrap().to_string();
		(tmp, path_str)
	}

	async fn get_operation(dem_path: &str, extra_args: &str) -> Operation {
		Operation::new(
			VPLNode::try_from_str(&format!(
				"from_gdal_dem filename=\"{dem_path}\" tile_size=\"256\" level_min=\"0\" level_max=\"2\" {extra_args}"
			))
			.unwrap(),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap()
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "from_gdal_dem");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("filename"));
		assert!(docs.contains("encoding"));
		assert!(docs.contains("tile_size"));
		assert!(docs.contains("level_max"));
		assert!(docs.contains("level_min"));
		assert!(docs.contains("cutline"));
		assert!(docs.contains("gdal_reuse_limit"));
		assert!(docs.contains("gdal_concurrency_limit"));
	}

	#[rstest]
	#[case("", DemEncoding::Mapbox)]
	#[case("encoding=\"mapbox\"", DemEncoding::Mapbox)]
	#[case("encoding=\"terrarium\"", DemEncoding::Terrarium)]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_encoding(#[case] extra_args: &str, #[case] expected_encoding: DemEncoding) {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, extra_args).await;
		assert_eq!(op.encoding, expected_encoding);
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_invalid_encoding() {
		let (_tmp, dem_path) = create_temp_dem();
		let result = Operation::new(
			VPLNode::try_from_str(&format!("from_gdal_dem filename=\"{dem_path}\" encoding=\"invalid\"")).unwrap(),
			&PipelineFactory::new_dummy(),
		)
		.await;
		assert!(result.is_err());
		let err = format!("{:#}", result.unwrap_err());
		assert!(err.contains("Unknown DEM encoding"), "got: {err}");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_metadata() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "").await;
		assert_eq!(*op.metadata.tile_format(), TileFormat::PNG);
		assert_eq!(*op.metadata.tile_compression(), TileCompression::Uncompressed);
		let pyramid = op.metadata.tile_pyramid().unwrap();
		assert_eq!(pyramid.level_min(), Some(0));
		assert_eq!(pyramid.level_max(), Some(2));
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_tilejson() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "").await;
		let tilejson = op.tilejson();
		assert!(tilejson.bounds.is_some());
		let bounds = tilejson.bounds.unwrap();
		assert!((bounds.x_min - 14.0).abs() < 0.1);
		assert!((bounds.y_min - 49.0).abs() < 0.1);
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_source_type() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "").await;
		assert_eq!(op.source_type().to_string(), "container 'gdal_dem' ('gdal_dem')");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_operation_tile_size() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "").await;
		assert_eq!(op.tile_size, 256);
	}

	#[rstest]
	#[case("")]
	#[case("encoding=\"terrarium\"")]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_tile_stream(#[case] extra_args: &str) -> Result<()> {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, extra_args).await;
		let bbox = TileBBox::new_full(1)?;
		let stream = op.tile_stream(bbox).await?;
		let tiles = stream.to_vec().await;
		assert!(!tiles.is_empty(), "stream should produce tiles");
		for (coord, tile) in &tiles {
			assert!(coord.level == 1);
			let image = tile.clone().into_image()?;
			assert_eq!(image.width(), 256);
			assert_eq!(image.height(), 256);
		}
		Ok(())
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_tile_stream_empty_bbox() -> Result<()> {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "").await;
		// Use a bbox that doesn't overlap the data (data is at lon 14-24, lat 49-55)
		let bbox = TileBBox::from_geo_bbox(1, &GeoBBox::new(-180.0, -85.0, -170.0, -80.0).unwrap())?;
		let stream = op.tile_stream(bbox).await?;
		let tiles = stream.to_vec().await;
		assert!(tiles.is_empty(), "stream should be empty for non-overlapping bbox");
		Ok(())
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_mapbox_tilejson_schema() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "encoding=\"mapbox\"").await;
		assert_eq!(op.tilejson().tile_schema, Some(TileSchema::RasterDEMMapbox));
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_terrarium_tilejson_schema() {
		let (_tmp, dem_path) = create_temp_dem();
		let op = get_operation(&dem_path, "encoding=\"terrarium\"").await;
		assert_eq!(op.tilejson().tile_schema, Some(TileSchema::RasterDEMTerrarium));
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn test_build_via_factory_trait() -> Result<()> {
		let (_tmp, dem_path) = create_temp_dem();
		let factory = PipelineFactory::new_dummy();
		let vpl_node = VPLNode::try_from_str(&format!(
			"from_gdal_dem filename=\"{dem_path}\" level_min=\"0\" level_max=\"1\""
		))
		.unwrap();
		let source = Operation::build(vpl_node, &factory).await?;
		assert_eq!(*source.metadata().tile_format(), TileFormat::PNG);
		Ok(())
	}
}
