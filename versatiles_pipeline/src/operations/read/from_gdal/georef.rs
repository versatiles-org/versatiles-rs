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
//! changing how every other tool reads that file afterwards.
//! [`open`](GeoreferenceOverride::open) suppresses GDAL's persistent auxiliary
//! metadata (PAM) around the datasets it is about to mutate, so the overrides
//! live only in memory, for the lifetime of the dataset handle.
//!
//! That suppression is kept as small as it can be, in two directions, because
//! PAM is also how GDAL *reads* a sidecar:
//!
//! * It is skipped when there is nothing to impose. A PNG or JPEG cannot carry
//!   a CRS itself, so `gdal_translate -a_srs` writes one to `<file>.aux.xml`;
//!   switching PAM off would hide the georeferencing of a dataset nobody asked
//!   to override (#261).
//! * It is thread-local rather than process-global, so a pipeline that does
//!   override a raster does not change how GDAL behaves for every other dataset
//!   in the host process — including code with no connection to VersaTiles.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};
use gdal::{
	Dataset, GeoTransform,
	config::{clear_thread_local_config_option, set_thread_local_config_option},
};
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
/// let [`open`](Self::open) open each dataset the factory needs.
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

	/// Opens `path` with the override already imposed on the dataset.
	///
	/// The dataset factories call this instead of opening the file themselves,
	/// because whether GDAL's sidecar handling has to be suppressed depends on
	/// whether this override is going to mutate the dataset — and the guard has
	/// to be alive across the open, not just across the mutation. See the
	/// module documentation.
	///
	/// Called once per dataset the pool opens, so it must stay cheap; an
	/// override that imposes nothing costs one comparison and a plain open.
	#[context("Failed to open GDAL dataset {:?}", path)]
	pub fn open(&self, path: &Path) -> Result<Dataset> {
		if self.is_empty() {
			return Ok(Dataset::open(path)?);
		}

		let _pam = PamSuppressed::new()?;
		let mut dataset = Dataset::open(path)?;
		self.apply(&mut dataset)?;
		Ok(dataset)
	}

	/// Whether the override has anything to impose.
	fn is_empty(&self) -> bool {
		self.crs.is_none() && self.placement.is_none()
	}

	/// Imposes the override on a freshly opened dataset.
	///
	/// Private because it is only half the job: PAM has to be suppressed before
	/// the dataset is opened, which is what [`open`](Self::open) is for.
	#[context("Failed to apply the georeferencing override")]
	fn apply(&self, dataset: &mut Dataset) -> Result<()> {
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

/// Stops GDAL persisting our in-memory overrides to the user's filesystem, for
/// as long as the guard is held on this thread.
///
/// Writing either the CRS or the geotransform of a read-only dataset makes GDAL
/// save the change into a `.aux.xml` sidecar beside the input file, where it
/// then overrides that file's georeferencing for every other tool — QGIS,
/// `gdalinfo`, a later VersaTiles run. Reading a dataset must not rewrite it, so
/// the guard goes here, next to the only code that mutates one.
///
/// `GDAL_PAM_ENABLED` is read when a dataset initialises its PAM state, which
/// happens while the dataset is being opened, so the guard has to span the open
/// and not merely the mutation that follows it. Thread-local rather than
/// global: a dataset opened at the same moment on another thread — by another
/// pipeline, or by another library entirely — goes on reading its sidecars.
struct PamSuppressed;

impl PamSuppressed {
	fn new() -> Result<Self> {
		set_thread_local_config_option("GDAL_PAM_ENABLED", "NO")
			.context("failed to disable GDAL's persistent auxiliary metadata")?;
		log::trace!("GDAL_PAM_ENABLED set to NO for this thread");
		Ok(Self)
	}
}

impl Drop for PamSuppressed {
	fn drop(&mut self) {
		if let Err(error) = clear_thread_local_config_option("GDAL_PAM_ENABLED") {
			log::warn!("failed to restore GDAL_PAM_ENABLED: {error}");
		}
	}
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

	/// A PNG whose CRS can only live in a `.aux.xml` sidecar, because the
	/// format cannot carry one itself. Its extent comes from the world file
	/// beside it; delete the sidecar and the extent survives while the CRS does
	/// not, which is the shape of the dataset #261 was reported for.
	const SIDECAR: &str = "../testdata/sidecar_georeferenced.png";

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
	/// tool. Without [`PamSuppressed`] this test finds one next to
	/// `gradient.tif`, which is how it earns its keep.
	#[test]
	fn applying_an_override_writes_no_sidecar() {
		let source = std::path::Path::new("../testdata/gradient.tif");
		let sidecar = std::path::Path::new("../testdata/gradient.tif.aux.xml");
		let before = std::fs::read(source).expect("test fixture readable");

		let dataset = GeoreferenceOverride::parse(Some(25832), None, Some("400000,9.6,2.5,5610000,2.5,-9.6"))
			.unwrap()
			.open(source)
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

	/// The regression #261 was filed for: 4.11 switched PAM off for every
	/// dataset the pool opened, and PAM is how GDAL *reads* a `.aux.xml`
	/// sidecar as well as how it writes one. A dataset with nothing to
	/// override must be opened with GDAL's sidecar handling left alone.
	#[test]
	fn a_dataset_with_no_override_keeps_its_sidecar() {
		let dataset = GeoreferenceOverride::default()
			.open(Path::new(SIDECAR))
			.expect("the fixture opens");

		let srs = dataset
			.spatial_ref()
			.expect("the CRS is in the sidecar, so opening must not disable PAM");
		assert_eq!(srs.auth_code().unwrap(), 4326);
	}

	/// The blast radius the same issue demonstrated: because the option used to
	/// be process-global and latched behind a `Once`, one overridden raster
	/// stopped *every* later dataset in the process from seeing its sidecar —
	/// including datasets opened by code with no connection to VersaTiles.
	///
	/// Both halves run on this thread, in this order, so the assertion below
	/// fails if the suppression outlives the open it was needed for.
	#[test]
	fn an_override_does_not_hide_another_datasets_sidecar() {
		let overridden = GeoreferenceOverride::parse(Some(25832), None, None)
			.unwrap()
			.open(Path::new("../testdata/gradient.tif"))
			.unwrap();
		assert_eq!(overridden.spatial_ref().unwrap().auth_code().unwrap(), 25832);
		drop(overridden);

		let untouched = Dataset::open(Path::new(SIDECAR)).unwrap();
		assert_eq!(
			untouched.spatial_ref().map(|srs| srs.auth_code().unwrap()).ok(),
			Some(4326),
			"overriding one dataset must not change how GDAL reads another"
		);
	}

	/// `bounds` is shorthand for a north-up geotransform derived from the
	/// raster's own pixel dimensions, the way `gdal_translate -a_ullr` is.
	#[test]
	fn bounds_derives_a_north_up_transform_from_the_raster_size() {
		let path = std::path::Path::new("../testdata/gradient.tif");
		assert_eq!(
			Dataset::open(path).unwrap().raster_size(),
			(256, 256),
			"fixture size, which the maths below assumes"
		);

		let dataset = GeoreferenceOverride::parse(None, Some("0,0,256,128"), None)
			.unwrap()
			.open(path)
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
