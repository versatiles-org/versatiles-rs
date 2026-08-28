//! Manual georeferencing for GDAL datasets that lack it, or have it wrong.
//!
//! A scanned map, a plain PNG export, or a raster whose world file went missing
//! carries no usable georeferencing, and GDAL cannot place such a dataset at
//! all. The `from_gdal_*` operations therefore accept the missing pieces as
//! arguments and impose them on every dataset the pool opens.
//!
//! # Two spellings, one mechanism
//!
//! GDAL places a raster with a single six-number geotransform. `bounds` is the
//! north-up shorthand that derives those six from four corner coordinates and
//! the raster's own pixel dimensions, exactly as `gdal_translate -a_ullr` does:
//!
//! ```text
//! GT[0] = west                       GT[1] = (east − west) / raster_width
//! GT[2] = 0                          GT[3] = north
//! GT[4] = 0                          GT[5] = (south − north) / raster_height
//! ```
//!
//! Both lower to [`Dataset::set_geo_transform`], so the shorthand cannot drift
//! from the general form — it *is* the general form, with two of the six
//! coefficients pinned to zero and two derived. Rotated and skewed rasters need
//! the full six, which is why both spellings exist.
//!
//! # Why this does not write to the user's files
//!
//! Overriding either the CRS or the geotransform of a *read-only* dataset makes
//! GDAL persist the change to a `.aux.xml` sidecar next to the input, silently
//! changing how every other tool reads that file afterwards. `GdalPool` sets
//! `GDAL_PAM_ENABLED=NO` before opening anything to stop that; the overrides
//! then live only in memory, for the lifetime of the dataset handle.

use std::sync::Once;

use anyhow::{Context, Result, anyhow, bail, ensure};
use gdal::{Dataset, GeoTransform, config::set_config_option};
use versatiles_derive::context;

use super::get_spatial_ref;

/// How the caller chose to place the raster.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Placement {
	/// `west, south, east, north` in the units of the dataset's CRS, north-up.
	Bounds([f64; 4]),
	/// The six GDAL geotransform coefficients, used verbatim.
	Transform(GeoTransform),
}

/// Georeferencing to impose on each dataset as it is opened.
///
/// Build one with [`parse`](Self::parse) from the operation's arguments, then
/// call [`apply`](Self::apply) inside the dataset factory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoreferenceOverride {
	crs: Option<u32>,
	placement: Option<Placement>,
}

impl GeoreferenceOverride {
	/// Reads the `crs`, `bounds` and `geo_transform` arguments.
	///
	/// # Errors
	///
	/// Errors when `bounds` and `geo_transform` are both given, when either
	/// holds the wrong count of numbers or something unparseable, when `bounds`
	/// is inverted, or when `geo_transform` has a zero pixel size.
	pub fn parse(crs: Option<u32>, bounds: Option<&str>, geo_transform: Option<&str>) -> Result<Self> {
		let placement = match (bounds, geo_transform) {
			(Some(_), Some(_)) => {
				bail!("`bounds` and `geo_transform` are two spellings of the same placement, so only one may be given")
			}
			(Some(text), None) => Some(Placement::Bounds(parse_bounds(text)?)),
			(None, Some(text)) => Some(Placement::Transform(parse_geo_transform(text)?)),
			(None, None) => None,
		};
		Ok(Self { crs, placement })
	}

	/// Imposes the override on a freshly opened dataset.
	///
	/// Called once per dataset the pool opens, so it must stay cheap.
	#[context("Failed to apply the georeferencing override")]
	pub fn apply(&self, dataset: &mut Dataset) -> Result<()> {
		disable_pam()?;

		if let Some(epsg) = self.crs {
			let srs = get_spatial_ref(epsg)?;
			dataset
				.set_spatial_ref(&srs)
				.with_context(|| format!("failed to set the CRS to EPSG:{epsg}"))?;
		}

		if let Some(placement) = self.placement {
			let transform = match placement {
				Placement::Transform(transform) => transform,
				Placement::Bounds(bounds) => bounds_to_transform(bounds, dataset)?,
			};
			dataset
				.set_geo_transform(&transform)
				.with_context(|| format!("failed to set the geotransform to {transform:?}"))?;
		}

		Ok(())
	}
}

/// Stops GDAL persisting our in-memory overrides to the user's filesystem.
///
/// Writing either the CRS or the geotransform of a read-only dataset makes GDAL
/// save the change into a `.aux.xml` sidecar beside the input file, where it
/// then overrides that file's georeferencing for every other tool — QGIS,
/// `gdalinfo`, a later VersaTiles run. Reading a dataset must not rewrite it, so
/// the setting goes here, next to the only code that mutates one.
///
/// The option is process-global, hence the [`Once`]; it is set before any
/// override is applied, which is the only point at which it matters.
fn disable_pam() -> Result<()> {
	static DISABLE: Once = Once::new();
	let mut result = Ok(());
	DISABLE.call_once(|| {
		result = set_config_option("GDAL_PAM_ENABLED", "NO").map_err(anyhow::Error::from);
		log::trace!("GDAL_PAM_ENABLED set to NO");
	});
	result.context("failed to disable GDAL's persistent auxiliary metadata")
}

/// Derives a north-up geotransform from corner coordinates and the raster's
/// own pixel dimensions, the way `gdal_translate -a_ullr` does.
fn bounds_to_transform([west, south, east, north]: [f64; 4], dataset: &Dataset) -> Result<GeoTransform> {
	let (width, height) = dataset.raster_size();
	ensure!(
		width > 0 && height > 0,
		"`bounds` needs a raster with a non-zero size, but this one is {width}x{height}"
	);

	Ok([
		west,
		(east - west) / width as f64,
		0.0,
		north,
		0.0,
		(south - north) / height as f64,
	])
}

/// Splits `text` on commas into exactly `N` numbers.
fn parse_numbers<const N: usize>(field: &str, text: &str) -> Result<[f64; N]> {
	let values = text
		.split(',')
		.map(|value| {
			value
				.trim()
				.parse::<f64>()
				.map_err(|_| anyhow!("`{field}` contains '{}', which is not a number", value.trim()))
		})
		.collect::<Result<Vec<f64>>>()?;

	<[f64; N]>::try_from(values.as_slice()).map_err(|_| {
		anyhow!(
			"`{field}` needs {N} comma-separated numbers, but '{text}' has {}",
			values.len()
		)
	})
}

/// Parses `west,south,east,north`, rejecting an inverted box.
fn parse_bounds(text: &str) -> Result<[f64; 4]> {
	let bounds: [f64; 4] = parse_numbers("bounds", text)?;
	let [west, south, east, north] = bounds;

	// An inverted box mirrors the image rather than failing, so it is caught
	// here instead of producing a map that is silently back-to-front.
	ensure!(
		east > west,
		"`bounds` needs east > west, but got west={west}, east={east}"
	);
	ensure!(
		north > south,
		"`bounds` needs north > south, but got south={south}, north={north}"
	);

	Ok(bounds)
}

/// Parses the six GDAL coefficients, rejecting a degenerate pixel size.
fn parse_geo_transform(text: &str) -> Result<GeoTransform> {
	let transform: GeoTransform = parse_numbers("geo_transform", text)?;

	// GDAL accepts a zero pixel size and then renders nothing from it.
	ensure!(
		transform[1] != 0.0,
		"`geo_transform` needs a non-zero pixel width as its 2nd number, but got {}",
		transform[1]
	);
	ensure!(
		transform[5] != 0.0,
		"`geo_transform` needs a non-zero pixel height as its 6th number, but got {}",
		transform[5]
	);

	Ok(transform)
}

#[cfg(test)]
#[expect(
	clippy::float_cmp,
	reason = "the geotransforms below are set and read back without arithmetic, so they compare exactly"
)]
mod tests {
	use super::*;

	#[test]
	fn bounds_and_geo_transform_are_mutually_exclusive() {
		let error = GeoreferenceOverride::parse(None, Some("0,0,1,1"), Some("0,1,0,1,0,-1"))
			.expect_err("giving both placements is an error");
		assert!(error.to_string().contains("only one may be given"), "{error}");
	}

	#[test]
	fn nothing_given_leaves_the_dataset_alone() {
		let empty = GeoreferenceOverride::parse(None, None, None).unwrap();
		assert_eq!(empty, GeoreferenceOverride::default());
	}

	#[rstest::rstest]
	#[case::too_few("0,0,1", "needs 4 comma-separated numbers")]
	#[case::too_many("0,0,1,1,2", "needs 4 comma-separated numbers")]
	#[case::not_a_number("0,0,1,north", "which is not a number")]
	#[case::inverted_x("1,0,0,1", "needs east > west")]
	#[case::inverted_y("0,1,1,0", "needs north > south")]
	fn bounds_rejects(#[case] text: &str, #[case] expected: &str) {
		let error = GeoreferenceOverride::parse(None, Some(text), None).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	#[rstest::rstest]
	#[case::too_few("0,1,0,1,0", "needs 6 comma-separated numbers")]
	#[case::zero_pixel_width("0,0,0,1,0,-1", "non-zero pixel width")]
	#[case::zero_pixel_height("0,1,0,1,0,0", "non-zero pixel height")]
	fn geo_transform_rejects(#[case] text: &str, #[case] expected: &str) {
		let error = GeoreferenceOverride::parse(None, None, Some(text)).expect_err("should be rejected");
		assert!(error.to_string().contains(expected), "got: {error}");
	}

	/// Applying an override must not touch the user's data directory.
	///
	/// GDAL persists metadata written to a read-only dataset into a `.aux.xml`
	/// sidecar, which then overrides that file's georeferencing for every other
	/// tool. Without `disable_pam` this test finds one next to `gradient.tif`.
	#[test]
	fn applying_an_override_writes_no_sidecar() {
		let source = std::path::Path::new("../testdata/gradient.tif");
		let sidecar = std::path::Path::new("../testdata/gradient.tif.aux.xml");
		let before = std::fs::read(source).expect("test fixture readable");

		let mut dataset = Dataset::open(source).unwrap();
		GeoreferenceOverride::parse(Some(25832), None, Some("400000,9.6,2.5,5610000,2.5,-9.6"))
			.unwrap()
			.apply(&mut dataset)
			.unwrap();
		assert_eq!(
			dataset.geo_transform().unwrap(),
			[400000.0, 9.6, 2.5, 5610000.0, 2.5, -9.6],
			"the override applies in memory"
		);
		drop(dataset);

		assert!(
			!sidecar.exists(),
			"GDAL wrote {} — PAM is not disabled",
			sidecar.display()
		);
		assert_eq!(before, std::fs::read(source).unwrap(), "the source file is unchanged");
	}

	/// `bounds` is shorthand for a north-up geotransform derived from the
	/// raster's own pixel dimensions, the way `gdal_translate -a_ullr` is.
	#[test]
	fn bounds_derives_a_north_up_transform_from_the_raster_size() {
		let mut dataset = Dataset::open(std::path::Path::new("../testdata/gradient.tif")).unwrap();
		let (width, height) = dataset.raster_size();
		assert_eq!(
			(width, height),
			(256, 256),
			"fixture size, which the maths below assumes"
		);

		GeoreferenceOverride::parse(None, Some("0,0,256,128"), None)
			.unwrap()
			.apply(&mut dataset)
			.unwrap();

		// 256 px across 256 units → 1.0/px; 256 px down 128 units → -0.5/px.
		assert_eq!(dataset.geo_transform().unwrap(), [0.0, 1.0, 0.0, 128.0, 0.0, -0.5]);
	}

	#[test]
	fn whitespace_around_numbers_is_accepted() {
		let parsed = GeoreferenceOverride::parse(None, Some(" 0 , 0 , 10 , 10 "), None).unwrap();
		assert_eq!(parsed.placement, Some(Placement::Bounds([0.0, 0.0, 10.0, 10.0])));
	}
}
