use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use gdal::{Dataset, DriverManager, GeoTransform};
use imageproc::image::{DynamicImage, RgbaImage};
use versatiles_core::{CancelToken, GeoBBox};
use versatiles_derive::context;

use super::{
	Cutline, GdalPool, GeoreferenceOverride, Instance, ResampleAlg, get_spatial_ref, stop_warp_when_cancelled,
};
/// DEM encoding format.
///
/// Re-exported rather than redeclared: `encoding=` on `from_gdal_dem` and on the
/// `dem_*` transforms must accept exactly the same spellings, and `VPLDecode`
/// renders that list from the enum's own `variants()`. Two enums would be two
/// lists that could drift.
pub use crate::operations::dem::encoding::DemEncoding;

/// Encode an elevation value (in meters) into an RGB triplet.
///
/// **Mapbox Terrain-RGB**: `raw = round((elevation + 10000) / 0.1)`
/// **Terrarium**: `raw = round((elevation + 32768) * 256)`
///
/// Both clamp to `[0, 0x00FFFFFF]` (24-bit range).
pub fn encode_elevation(elevation: f32, encoding: DemEncoding) -> [u8; 3] {
	let raw = match encoding {
		DemEncoding::Mapbox => ((f64::from(elevation) + 10000.0) * 10.0).round(),
		DemEncoding::Terrarium => ((f64::from(elevation) + 32768.0) * 256.0).round(),
	};
	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "clamped to 0..=0xFF_FFFF on the same line"
	)]
	let raw = (raw as i64).clamp(0, 0x00FF_FFFF) as u32;
	[
		((raw >> 16) & 0xFF) as u8,
		((raw >> 8) & 0xFF) as u8,
		(raw & 0xFF) as u8,
	]
}

/// Reproject the source dataset into a 2-band Float32 in-memory dataset
/// covering the given bbox in Web Mercator (EPSG:3857).
///
/// Band 1 holds the elevation and band 2 how much of each pixel the source
/// covers, from `0` to `255`. A pixel outside the source, outside the cutline
/// or on the source's `nodata` value has no elevation at all, and band 2 is
/// what says so: band 1 is `0` there, which is a height like any other.
#[expect(
	unsafe_code,
	reason = "GDAL warp through its C API; the handles are owned by this function and destroyed on every path"
)]
fn reproject_to_float_dataset(
	instance: &Instance,
	width: usize,
	height: usize,
	bbox: &GeoBBox,
	cutline: Option<&Cutline>,
	nodata: Option<f64>,
) -> Result<Dataset> {
	log::trace!("reproject_to_float_dataset started for size={width}x{height}");
	// Kept until the warp is done: GDAL holds a pointer to it.
	let cancel = CancelToken::current();
	cancel.check()?;

	let bbox_mer = bbox.to_mercator();

	// Create 2-band Float32 MEM dataset: elevation and coverage
	let driver = DriverManager::get_driver_by_name("MEM").context("Failed to get GDAL MEM driver")?;
	let mut dst_ds = driver
		.create_with_band_type::<f32, _>("mem", width, height, 2)
		.context("Failed to create Float32 in-memory dataset")?;
	dst_ds.set_spatial_ref(&get_spatial_ref(3857)?)?;

	let geo_transform: GeoTransform = [
		bbox_mer[0],
		(bbox_mer[2] - bbox_mer[0]) / width as f64,
		0.0,
		bbox_mer[3],
		0.0,
		(bbox_mer[1] - bbox_mer[3]) / height as f64,
	];
	dst_ds.set_geo_transform(&geo_transform)?;

	let h_src_ds = instance.dataset().c_dataset();
	let h_dst_ds = dst_ds.c_dataset();

	// Create cutline geometry from WKT — must live until after warp completes
	let cutline_geom = cutline.map(Cutline::create_ogr_geometry).transpose()?;

	unsafe {
		use gdal_sys::{
			CPLErr, CPLGetLastErrorMsg, CPLMalloc, CSLSetNameValue, GDALChunkAndWarpMulti,
			GDALCreateGenImgProjTransformer2, GDALCreateWarpOperation, GDALCreateWarpOptions,
			GDALDestroyGenImgProjTransformer, GDALDestroyWarpOperation, GDALDestroyWarpOptions, GDALGenImgProjTransform,
			GDALWarpOperationH,
		};

		let options_ptr = GDALCreateWarpOptions();
		(*options_ptr).hSrcDS = h_src_ds;
		(*options_ptr).hDstDS = h_dst_ds;

		(*options_ptr).papszWarpOptions = CSLSetNameValue(
			(*options_ptr).papszWarpOptions,
			c"NUM_THREADS".as_ptr(),
			c"ALL_CPUS".as_ptr(),
		);

		// Band mapping: source band 1 -> dest band 1
		(*options_ptr).nBandCount = 1;
		let n = std::mem::size_of::<i32>();
		(*options_ptr).panSrcBands = CPLMalloc(n).cast::<i32>();
		(*options_ptr).panDstBands = CPLMalloc(n).cast::<i32>();
		(*options_ptr).panSrcBands.write(1);
		(*options_ptr).panDstBands.write(1);

		// Coverage goes to band 2, on the same 0..255 scale as an image's
		// alpha. GDAL's default maximum for a Float32 band is 255 already;
		// stating it keeps that from depending on the GDAL version.
		(*options_ptr).nDstAlphaBand = 2;
		(*options_ptr).papszWarpOptions = CSLSetNameValue(
			(*options_ptr).papszWarpOptions,
			c"DST_ALPHA_MAX".as_ptr(),
			c"255".as_ptr(),
		);

		// The low-level warp, unlike `gdalwarp`, does not read the source's
		// nodata value itself. Without it a void (often -32768) was warped as
		// a height and came out as the lowest the encoding can store.
		if let Some(value) = nodata {
			let nodata_arr = CPLMalloc(std::mem::size_of::<f64>()).cast::<f64>();
			nodata_arr.write(value);
			(*options_ptr).padfSrcNoDataReal = nodata_arr;
		}

		// Use Bilinear for DEM — preserves elevation values better than averaging
		(*options_ptr).eResampleAlg = ResampleAlg::Bilinear.as_gdal();
		(*options_ptr).dfWarpMemoryLimit = 512.0 * 1024.0 * 1024.0;

		if let Some(ref geom) = cutline_geom {
			(*options_ptr).hCutline = geom.c_geometry();
		}

		(*options_ptr).pTransformerArg = GDALCreateGenImgProjTransformer2(h_src_ds, h_dst_ds, core::ptr::null_mut());
		(*options_ptr).pfnTransformer = Some(GDALGenImgProjTransform);
		stop_warp_when_cancelled(options_ptr, &cancel);

		let operation: GDALWarpOperationH = GDALCreateWarpOperation(options_ptr);

		let rv = GDALChunkAndWarpMulti(
			operation,
			0,
			0,
			i32::try_from(width).context("warp width exceeds i32::MAX")?,
			i32::try_from(height).context("warp height exceeds i32::MAX")?,
		);

		GDALDestroyWarpOperation(operation);
		GDALDestroyGenImgProjTransformer((*options_ptr).pTransformerArg);
		GDALDestroyWarpOptions(options_ptr);

		if rv != CPLErr::CE_None {
			// Stopped by the token, not failed: say so rather than "User terminated".
			cancel.check()?;
			bail!("{:?}", CPLGetLastErrorMsg());
		}
	}
	// cutline_geom dropped here — GDAL only borrows hCutline during warp

	log::trace!("reproject_to_float_dataset complete");
	Ok(dst_ds)
}

/// Encode elevations as terrain RGB, with the coverage as alpha.
///
/// A pixel without coverage gets the colour of `0` m, as it had before it was
/// marked transparent, so that a reader that ignores alpha sees no change at
/// the edge of the data, and in a void `0` m rather than the lowest height
/// the encoding can store.
fn elevation_image(
	width: u32,
	height: u32,
	elevation: &[f32],
	coverage: &[f32],
	encoding: DemEncoding,
) -> Result<RgbaImage> {
	let none = encode_elevation(0.0, encoding);
	let mut buf = Vec::with_capacity(elevation.len() * 4);
	for (&elev, &cover) in elevation.iter().zip(coverage) {
		#[expect(
			clippy::cast_possible_truncation,
			clippy::cast_sign_loss,
			reason = "clamped to 0..=255 before the cast"
		)]
		let alpha = cover.round().clamp(0.0, 255.0) as u8;
		let [r, g, b] = if alpha == 0 {
			none
		} else {
			encode_elevation(elev, encoding)
		};
		buf.extend_from_slice(&[r, g, b, alpha]);
	}
	RgbaImage::from_raw(width, height, buf).context("Failed to create RgbaImage from elevation data")
}

#[derive(Clone)]
pub struct DemSource {
	pool: GdalPool,
	cutline: Option<Cutline>,
	/// The `nodata` value of the source's first band, if it declares one.
	nodata: Option<f64>,
}

impl DemSource {
	#[context("Failed to create DemSource from file {:?}", filename)]
	pub async fn new(
		filename: &Path,
		reuse_limit: u32,
		concurrency_limit: usize,
		cutline_path: Option<&Path>,
		georeference: GeoreferenceOverride,
	) -> Result<DemSource> {
		let path = filename.to_path_buf();
		let factory: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> =
			Arc::new(move || georeference.open(&path));
		Self::new_with_factory(factory, reuse_limit, concurrency_limit, cutline_path).await
	}

	#[context("Failed to create DemSource via factory")]
	pub async fn new_with_factory(
		open_dataset: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static>,
		reuse_limit: u32,
		concurrency_limit: usize,
		cutline_path: Option<&Path>,
	) -> Result<DemSource> {
		let (pool, probe) = GdalPool::new_with_factory(open_dataset, reuse_limit, concurrency_limit).await?;

		let cutline = if let Some(path) = cutline_path {
			let srs = probe
				.spatial_ref()
				.context("GDAL dataset must have a spatial reference for cutline support")?;
			Some(Cutline::from_geojson(path, &srs)?)
		} else {
			None
		};

		let nodata = probe.rasterband(1)?.no_data_value();

		Ok(DemSource { pool, cutline, nodata })
	}

	#[context("Failed to get elevation tile ({width}x{height}) for bbox ({bbox:?}) from GDAL DEM dataset")]
	pub async fn elevation_tile(
		&self,
		bbox: &GeoBBox,
		width: usize,
		height: usize,
		encoding: DemEncoding,
	) -> Result<Option<DynamicImage>> {
		// Opening a dataset can take a long time, and nothing stops it once
		// started, so ask before. The token is captured here because the warp
		// below runs on the blocking pool, which does not inherit it.
		let cancel = CancelToken::current();
		cancel.check()?;

		let instance = self.pool.instance().await?;
		let cutline = self.cutline.clone();
		let nodata = self.nodata;
		let bbox = *bbox;

		// The warp, too, runs on the blocking pool, as in `RasterSource::image`.
		// It used to run here on the async executor, where it held a worker
		// thread for as long as the warp took.
		let image = versatiles_core::stage::spawn_blocking(move || {
			cancel.sync_scope(|| -> Result<Option<DynamicImage>> {
				let dst = reproject_to_float_dataset(&instance, width, height, &bbox, cutline.as_ref(), nodata)?;
				let elevation = dst.rasterband(1)?.read_band_as::<f32>()?;
				let coverage = dst.rasterband(2)?.read_band_as::<f32>()?;
				let (elevation, coverage) = (elevation.data(), coverage.data());
				ensure!(
					elevation.len() == width * height && coverage.len() == width * height,
					"Band data length mismatch: expected {} but got {} and {}",
					width * height,
					elevation.len(),
					coverage.len()
				);

				Ok(Some(DynamicImage::ImageRgba8(elevation_image(
					u32::try_from(width)?,
					u32::try_from(height)?,
					elevation,
					coverage,
					encoding,
				)?)))
			})
		})
		.await??;

		Ok(image)
	}

	pub fn bbox(&self) -> &GeoBBox {
		self.pool.bbox()
	}

	pub fn level_max(&self, tile_size: u32) -> Result<u8> {
		self.pool.level_max(tile_size)
	}

	pub fn cutline_bbox(&self) -> Option<&GeoBBox> {
		self.cutline.as_ref().map(Cutline::bbox_wgs84)
	}
}

impl std::fmt::Debug for DemSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DemSource").field("pool", &self.pool).finish()
	}
}

#[cfg(test)]
mod tests {
	use gdal::DriverManager;
	use rstest::rstest;

	use super::{super::get_spatial_ref, *};

	struct DemDatasetFactory {
		geotransform: [f64; 6],
		size: usize,
	}

	impl DemDatasetFactory {
		pub fn new(bbox: GeoBBox) -> Self {
			let size = 256;
			let geotransform = [
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			];
			DemDatasetFactory { geotransform, size }
		}

		pub fn factory(&self) -> Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> {
			let geotransform_c = self.geotransform;
			let size = self.size;
			Arc::new(move || -> Result<Dataset> {
				let driver = DriverManager::get_driver_by_name("MEM")?;
				let mut ds = driver.create_with_band_type::<f32, _>("in memory dem", size, size, 1)?;
				ds.set_spatial_ref(&get_spatial_ref(4326)?)?;
				ds.set_geo_transform(&geotransform_c)?;

				// Fill with a gradient elevation: 0..8848 meters
				let mut elev_data = vec![0.0f32; size * size];
				for row in 0..size {
					for col in 0..size {
						elev_data[row * size + col] = (col as f32 / size as f32) * 8848.0;
					}
				}
				let mut buffer = gdal::raster::Buffer::new((size, size), elev_data);
				ds.rasterband(1)?.write((0, 0), (size, size), &mut buffer)?;
				Ok(ds)
			})
		}
	}

	impl DemSource {
		pub fn from_testdata(bbox: GeoBBox) -> Result<DemSource> {
			let factory = DemDatasetFactory::new(bbox).factory();
			futures::executor::block_on(DemSource::new_with_factory(factory, 1, 2, None))
		}
	}

	#[test]
	fn test_encode_elevation_mapbox() {
		// 0m: raw = (0 + 10000) / 0.1 = 100000
		let [r, g, b] = encode_elevation(0.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 100_000);

		// 100m: raw = (100 + 10000) / 0.1 = 101000
		let [r, g, b] = encode_elevation(100.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 101_000);

		// -100m: raw = (-100 + 10000) / 0.1 = 99000
		let [r, g, b] = encode_elevation(-100.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 99_000);

		// 8848m (Everest): raw = (8848 + 10000) / 0.1 = 188480
		let [r, g, b] = encode_elevation(8848.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 188_480);
	}

	#[test]
	fn test_encode_elevation_terrarium() {
		// 0m: raw = (0 + 32768) * 256 = 8388608
		let [r, g, b] = encode_elevation(0.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_388_608);

		// 100m: raw = (100 + 32768) * 256 = 8414208
		let [r, g, b] = encode_elevation(100.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_414_208);

		// -100m: raw = (-100 + 32768) * 256 = 8363008
		let [r, g, b] = encode_elevation(-100.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 8_363_008);

		// 8848m: raw = (8848 + 32768) * 256 = 10653696
		let [r, g, b] = encode_elevation(8848.0, DemEncoding::Terrarium);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 10_653_696);
	}

	#[test]
	fn test_encode_elevation_clamping() {
		// Very negative: should clamp to 0
		let [r, g, b] = encode_elevation(-20000.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 0);

		// Very high: should clamp to 0x00FFFFFF
		let [r, g, b] = encode_elevation(2_000_000.0, DemEncoding::Mapbox);
		let raw = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
		assert_eq!(raw, 0x00FF_FFFF);
	}

	#[test]
	fn test_bbox() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let bbox_out = ds.bbox();
		assert!((bbox_out.x_min - 14.0).abs() < 0.001);
		assert!((bbox_out.y_min - 49.0).abs() < 0.001);
		assert!((bbox_out.x_max - 24.0).abs() < 0.001);
		assert!((bbox_out.y_max - 55.0).abs() < 0.001);
	}

	#[test]
	fn test_level_max() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let level = ds.level_max(256).unwrap();
		assert!(level > 0);
		assert!(level <= 31);
		let level_512 = ds.level_max(512).unwrap();
		assert!(level_512 <= level);
	}

	#[test]
	fn test_level_max_zero_tile_size_fails() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let result = ds.level_max(0);
		assert!(result.is_err());
	}

	#[rstest]
	#[case(DemEncoding::Mapbox)]
	#[case(DemEncoding::Terrarium)]
	#[tokio::test(flavor = "multi_thread")]
	async fn test_elevation_tile(#[case] encoding: DemEncoding) {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let image = ds.elevation_tile(&bbox_in, 256, 256, encoding).await.unwrap().unwrap();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
	}

	/// The token reaches the warp on the blocking pool, and its error says the
	/// request was cancelled rather than that it failed (#265).
	#[tokio::test(flavor = "multi_thread")]
	async fn a_cancelled_request_gets_no_elevation_tile() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();

		let cancel = CancelToken::new();
		cancel.cancel();
		let error = cancel
			.scope(ds.elevation_tile(&bbox_in, 256, 256, DemEncoding::Mapbox))
			.await
			.unwrap_err();
		assert!(versatiles_core::Cancelled::is_cause_of(&error), "{error:#}");

		// A request without a token is not affected.
		assert!(
			ds.elevation_tile(&bbox_in, 256, 256, DemEncoding::Mapbox)
				.await
				.unwrap()
				.is_some()
		);
	}

	/// A DEM of 100 m in its western half and voids, marked as `nodata`, in
	/// its eastern half, as SRTM marks them.
	fn dem_with_voids(bbox: GeoBBox) -> Result<DemSource> {
		let factory: Arc<dyn Fn() -> Result<Dataset> + Send + Sync + 'static> = Arc::new(move || {
			let size = 64;
			let driver = DriverManager::get_driver_by_name("MEM")?;
			let mut ds = driver.create_with_band_type::<f32, _>("dem with voids", size, size, 1)?;
			ds.set_spatial_ref(&get_spatial_ref(4326)?)?;
			ds.set_geo_transform(&[
				bbox.x_min,
				(bbox.x_max - bbox.x_min) / size as f64,
				0.0,
				bbox.y_max,
				0.0,
				(bbox.y_min - bbox.y_max) / size as f64,
			])?;
			let data: Vec<f32> = (0..size * size)
				.map(|i| if i % size < size / 2 { 100.0 } else { -32768.0 })
				.collect();
			let mut band = ds.rasterband(1)?;
			band.set_no_data_value(Some(-32768.0))?;
			band.write((0, 0), (size, size), &mut gdal::raster::Buffer::new((size, size), data))?;
			Ok(ds)
		});
		futures::executor::block_on(DemSource::new_with_factory(factory, 1, 2, None))
	}

	/// Height and alpha of the pixel in `column` of the middle row.
	fn mapbox_pixel(image: &DynamicImage, column: u32) -> (f64, u8) {
		let [red, green, blue, alpha] = image
			.as_rgba8()
			.expect("an RGBA image")
			.get_pixel(column, image.height() / 2)
			.0;
		let raw = (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue);
		(-10000.0 + f64::from(raw) * 0.1, alpha)
	}

	/// Outside the data there is no height, which is what alpha says. The
	/// colour is that of 0 m, as it was before, for readers that ignore alpha.
	#[tokio::test(flavor = "multi_thread")]
	async fn pixels_outside_the_data_are_transparent() {
		let data = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(data).unwrap();
		// Ten degrees of nothing on either side of the data.
		let wider = GeoBBox::new(4.0, 49.0, 34.0, 55.0).unwrap();
		let image = ds
			.elevation_tile(&wider, 300, 60, DemEncoding::Mapbox)
			.await
			.unwrap()
			.unwrap();

		for x in [0, 50, 250, 299] {
			assert_eq!(mapbox_pixel(&image, x), (0.0, 0), "column {x} is outside the data");
		}
		let (height, alpha) = mapbox_pixel(&image, 150);
		assert_eq!(alpha, 255, "column 150 is inside the data");
		assert!((4000.0..5000.0).contains(&height), "about half of 8848 m, got {height}");
	}

	/// A void used to be warped as a height of -32768 m and stored as the
	/// lowest the encoding has, -10000 m.
	#[tokio::test(flavor = "multi_thread")]
	async fn a_void_is_transparent_rather_than_the_lowest_height() {
		let bbox = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = dem_with_voids(bbox).unwrap();
		let image = ds
			.elevation_tile(&bbox, 64, 64, DemEncoding::Mapbox)
			.await
			.unwrap()
			.unwrap();

		assert_eq!(mapbox_pixel(&image, 10), (100.0, 255));
		assert_eq!(mapbox_pixel(&image, 54), (0.0, 0));
	}

	#[test]
	fn test_debug() {
		let bbox_in = GeoBBox::new(14.0, 49.0, 24.0, 55.0).unwrap();
		let ds = DemSource::from_testdata(bbox_in).unwrap();
		let debug_str = format!("{ds:?}");
		assert!(debug_str.contains("DemSource"));
		assert!(debug_str.contains("bbox"));
		assert!(debug_str.contains("pixel_size"));
	}
}
